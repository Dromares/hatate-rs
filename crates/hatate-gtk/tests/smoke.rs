//! The window, headless: every scenario builds the real `MainWindow` over a temp profile with a
//! scripted prompter, drives it through its `gio::Action`s and model calls, and reads what it did
//! back. One program, because GTK must run on the main thread and the default test harness does
//! not give a test that.
//!
//! Needs a display. It runs under `xvfb-run` with `HATATE_GTK_SMOKE=1`. **Fails closed**: with
//! `CI` set, a missing flag or a missing display is a failure, not a skip (DAN-1974 item 1). Key
//! injection additionally needs `HATATE_SMOKE_XVFB=1` and an `xvfb-run` display (DAN-2264).

mod common;

use common::*;
use gtk4::gio;
use gtk4::prelude::*;
use hatate_core::engines::saucenao::SauceNaoQuota;
use hatate_core::model::MatchStatus;
use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile};
use hatate_core::session::{LoadOptions, SessionStore, named};
use hatate_core::table_menu::Action;
use hatate_gtk::dialogs::{Asked, Buttons, Kind, Response};
use hatate_gtk::menu::{self, Owner};
use hatate_gtk::shell::{EMPTY_GHOST_GLYPH, IDLE_ETA, IDLE_PROGRESS, RUN_FAILED_NOTE};
use libadwaita as adw;
use std::cell::Cell;
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

type Scenario = (&'static str, fn(&adw::Application));

const SCENARIOS: &[Scenario] = &[
    (
        "window_menus_and_deferred_actions",
        window_menus_and_deferred_actions,
    ),
    ("empty_state_and_idle_strip", empty_state_and_idle_strip),
    (
        "run_strip_and_sent_count_follow_the_list",
        run_strip_and_sent_count_follow_the_list,
    ),
    ("add_files_hashes_and_dedupes", add_files_hashes_and_dedupes),
    (
        "add_folder_scans_and_drop_filters",
        add_folder_scans_and_drop_filters,
    ),
    (
        "a_second_add_waits_and_the_window_keeps_working",
        a_second_add_waits_and_the_window_keeps_working,
    ),
    ("restore_clean_session", restore_clean_session),
    (
        "restore_after_a_crash_shows_the_banner",
        restore_after_a_crash_shows_the_banner,
    ),
    ("nothing_survived_variant", nothing_survived_variant),
    ("missing_files_dialog", missing_files_dialog),
    ("save_session_as", save_session_as),
    ("open_session", open_session),
    (
        "clear_session_matches_the_empty_list_quirk",
        clear_session_matches_the_empty_list_quirk,
    ),
    ("persisted_quota_pause", persisted_quota_pause),
    (
        "theme_action_saves_the_setting",
        theme_action_saves_the_setting,
    ),
    (
        "close_saves_and_marks_a_clean_exit",
        close_saves_and_marks_a_clean_exit,
    ),
    (
        "close_writes_an_empty_list_over_the_old_rows",
        close_writes_an_empty_list_over_the_old_rows,
    ),
    (
        "autosave_writes_the_list_off_the_main_thread",
        autosave_writes_the_list_off_the_main_thread,
    ),
    ("log_viewer_and_open_log", log_viewer_and_open_log),
    (
        "fonts_cover_the_status_glyphs_and_kanji",
        fonts_cover_the_status_glyphs_and_kanji,
    ),
    (
        "minimum_size_fits_every_state_in_both_schemes",
        minimum_size_fits_every_state_in_both_schemes,
    ),
    ("start_search_runs_the_batch", start_search_runs_the_batch),
    (
        "start_search_with_nothing_to_do_or_one_running",
        start_search_with_nothing_to_do_or_one_running,
    ),
    (
        "stop_search_finishes_the_image_under_way",
        stop_search_finishes_the_image_under_way,
    ),
    (
        "a_quota_pause_shows_the_banner_and_continue_resumes",
        a_quota_pause_shows_the_banner_and_continue_resumes,
    ),
    (
        "engine_alerts_reach_the_user_only_during_a_run",
        engine_alerts_reach_the_user_only_during_a_run,
    ),
    (
        "the_window_keeps_working_while_an_engine_stalls",
        the_window_keeps_working_while_an_engine_stalls,
    ),
    (
        "the_sites_menu_toggles_and_saves",
        the_sites_menu_toggles_and_saves,
    ),
    (
        "a_failed_run_is_shown_as_failed_not_as_a_crash",
        a_failed_run_is_shown_as_failed_not_as_a_crash,
    ),
    (
        "the_availability_check_offers_to_drop_the_dead_matches",
        the_availability_check_offers_to_drop_the_dead_matches,
    ),
    (
        "the_upscale_check_marks_rows_and_summarises",
        the_upscale_check_marks_rows_and_summarises,
    ),
    (
        "starting_a_search_clears_the_stopped_note",
        starting_a_search_clears_the_stopped_note,
    ),
    (
        "parser_health_dialog_and_warning",
        parser_health_dialog_and_warning,
    ),
    (
        "activity_page_states_the_engines_and_tails_the_log",
        activity_page_states_the_engines_and_tails_the_log,
    ),
    (
        "lens_connection_is_said_in_the_status_line",
        lens_connection_is_said_in_the_status_line,
    ),
    (
        "re_search_uses_the_engine_it_was_asked_for",
        re_search_uses_the_engine_it_was_asked_for,
    ),
    (
        "design_follow_ups_paint_as_specified",
        design_follow_ups_paint_as_specified,
    ),
    (
        "a_dialog_shows_the_ring_on_its_default",
        a_dialog_shows_the_ring_on_its_default,
    ),
    ("menubar_keyboard_path", menubar_keyboard_path),
    (
        "the_log_viewer_opens_on_close_and_selects_in_ink",
        the_log_viewer_opens_on_close_and_selects_in_ink,
    ),
    (
        "queue_view_holds_5000_rows_and_filters_without_touching_the_queue",
        queue_view_holds_5000_rows_and_filters_without_touching_the_queue,
    ),
    (
        "queue_filter_bar_narrows_by_text_and_menus_reads_hidden_and_clears",
        queue_filter_bar_narrows_by_text_and_menus_reads_hidden_and_clears,
    ),
    (
        "queue_select_by_status_under_a_filter_never_selects_a_hidden_row",
        queue_select_by_status_under_a_filter_never_selects_a_hidden_row,
    ),
    (
        "queue_clear_re_ticks_every_filter_menu_so_the_ticks_summary_and_count_agree",
        queue_clear_re_ticks_every_filter_menu_so_the_ticks_summary_and_count_agree,
    ),
    (
        "queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row",
        queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row,
    ),
    (
        "queue_view_thumbnails_load_fail_and_stay_off_the_main_loop",
        queue_view_thumbnails_load_fail_and_stay_off_the_main_loop,
    ),
    (
        "queue_view_thumbnails_stay_off_when_the_source_is_off",
        queue_view_thumbnails_stay_off_when_the_source_is_off,
    ),
    (
        "queue_header_layout_is_saved_on_close_and_restored_repaired_or_discarded",
        queue_header_layout_is_saved_on_close_and_restored_repaired_or_discarded,
    ),
    (
        "queue_header_titles_are_not_clipped_at_the_minimum_window_size",
        queue_header_titles_are_not_clipped_at_the_minimum_window_size,
    ),
    (
        "clear_search_cache_label_carries_the_cache_size",
        clear_search_cache_label_carries_the_cache_size,
    ),
    (
        "queue_context_menu_selects_marks_resets_and_removes",
        queue_context_menu_selects_marks_resets_and_removes,
    ),
    (
        "queue_view_header_cycle_sorts_the_queue_in_place_and_restores_the_order",
        queue_view_header_cycle_sorts_the_queue_in_place_and_restores_the_order,
    ),
    (
        "export_results_writes_the_format_the_name_says",
        export_results_writes_the_format_the_name_says,
    ),
    (
        "write_tag_files_asks_then_writes_and_keeps_what_is_there",
        write_tag_files_asks_then_writes_and_keeps_what_is_there,
    ),
    (
        "clear_search_cache_asks_then_removes_the_saved_results",
        clear_search_cache_asks_then_removes_the_saved_results,
    ),
];

fn main() {
    let ci = std::env::var_os("CI").is_some();
    let flag = std::env::var("HATATE_GTK_SMOKE").as_deref() == Ok("1");
    let display =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !flag {
        if ci {
            eprintln!("hatate-gtk smoke: CI is set but HATATE_GTK_SMOKE=1 is not: failing closed");
            std::process::exit(1);
        }
        println!("hatate-gtk smoke: skipped (set HATATE_GTK_SMOKE=1 under a display)");
        return;
    }
    if !display {
        eprintln!("hatate-gtk smoke: no DISPLAY or WAYLAND_DISPLAY: failing closed");
        std::process::exit(1);
    }
    // Before GTK starts: fontconfig reads its configuration once, at first use.
    let fonts = tempfile::tempdir().expect("font dir");
    hatate_gtk::fonts::load(fonts.path()).expect("the bundled fonts load");

    let failed = Rc::new(Cell::new(0usize));
    let f = failed.clone();
    let code = hatate_gtk::app::with_application(move |app| {
        // `HATATE_SMOKE_ONLY=<substring>` runs just the scenarios whose name contains it.
        let only = std::env::var("HATATE_SMOKE_ONLY").ok();
        for (name, scenario) in SCENARIOS {
            if only.as_deref().is_some_and(|o| !name.contains(o)) {
                continue;
            }
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| scenario(app)));
            match result {
                Ok(()) => println!("scenario {name} ... ok"),
                Err(e) => {
                    let why = e
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                        .unwrap_or_default();
                    println!("scenario {name} ... FAILED: {why}");
                    f.set(f.get() + 1);
                }
            }
        }
        app.quit();
    });
    let _ = code;
    if failed.get() > 0 {
        eprintln!("hatate-gtk smoke: {} scenario(s) failed", failed.get());
        std::process::exit(1);
    }
    println!("hatate-gtk smoke: {} scenarios ok", SCENARIOS.len());
}

fn fx(app: &adw::Application) -> Fx {
    Fx::new(app, Opts::default())
}

fn window_menus_and_deferred_actions(app: &adw::Application) {
    let f = fx(app);
    assert!(f.w.win.title().unwrap().contains("work in progress"));
    // Every action in the menus exists; a later ticket's is disabled, this ticket's is enabled.
    for (name, owner) in menu::actions() {
        if name == "site" {
            // The Check entries stand for one boolean action per site.
            for i in 0..menu::all_site_options().len() {
                let a = f.w.win.lookup_action(&menu::site_action(i));
                assert!(a.is_some_and(|a| a.is_enabled()), "site-{i}");
            }
            continue;
        }
        let action =
            f.w.win
                .lookup_action(name)
                .unwrap_or_else(|| panic!("no action {name}"));
        let enabled = action.is_enabled();
        match owner {
            Owner::Later(t) => assert!(!enabled, "{name} ({t}) must be disabled"),
            Owner::Shell => assert!(enabled, "{name} must be enabled"),
        }
    }
    // Start Search is wired (6.e): its action is enabled and the button follows it.
    assert!(
        f.w.win
            .lookup_action("toggle-search")
            .is_some_and(|a| a.is_enabled())
    );
    f.close();
}

/// Every label and button text under `root`, in tree order: what the screen says, as the user
/// reads it (the CSS `caps` class changes the paint, not the text).
fn texts_under(root: &gtk4::Widget) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(w) = stack.pop() {
        if let Some(l) = w.downcast_ref::<gtk4::Label>() {
            out.push(l.text().to_string());
        } else if let Some(b) = w.downcast_ref::<gtk4::Button>() {
            if let Some(t) = b.label() {
                out.push(t.to_string());
            }
        }
        let mut child = w.last_child();
        while let Some(c) = child {
            child = c.prev_sibling();
            stack.push(c);
        }
    }
    out
}

fn empty_state_and_idle_strip(app: &adw::Application) {
    let f = fx(app);
    // The empty state's copy, literally (`_build_drop_zone`, `screen_title("Queue.")`): the heading,
    // the section label and the three action labels the Python empty-state tests read.
    let said = texts_under(f.w.win.upcast_ref());
    for want in [
        "Queue.",
        "01 // Queue",
        "Drop images here.",
        "Add files\u{2026}",
        "Add folder\u{2026}",
        "Query Hydrus\u{2026}",
    ] {
        assert!(
            said.iter().any(|t| t == want),
            "the empty state does not say {want:?}: {said:?}"
        );
    }
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("empty")
    );
    assert_eq!(f.w.s.shell.root.glyph(), EMPTY_GHOST_GLYPH);
    assert_eq!(f.say(), "Ready");
    assert_eq!(f.w.s.shell.run_progress_label.text(), IDLE_PROGRESS);
    assert_eq!(f.w.s.shell.run_eta_label.text(), IDLE_ETA);
    assert!(!f.w.s.run_banner.root.is_visible());
    assert!(!f.w.s.quota_banner.root.is_visible());
    f.close();
}

/// `_refresh_sent_count_label`, `_idle_run_text` and the idle branch of
/// `_refresh_run_progress_label`: the strip's count and the status bar's `N sent` read the list
/// (found by the action differential: the first cut never refreshed them).
fn run_strip_and_sent_count_follow_the_list(app: &adw::Application) {
    let mut rows = vec![
        entry("/p/a.png", MatchStatus::NotSearched),
        entry("/p/b.png", MatchStatus::Good),
        entry("/p/c.png", MatchStatus::Poor),
        entry("/p/d.png", MatchStatus::NotFound),
    ];
    rows[1].sent_to_hydrus = true;
    rows[1].hydrus_import_confirmed = true;
    rows[2].sent_to_hydrus = true; // queued with Hydrus, not yet confirmed
    let f = Fx::new(
        app,
        Opts {
            seed: rows,
            ..Opts::default()
        },
    );
    let sh = &f.w.s.shell;
    assert_eq!(sh.run_progress_label.text(), "1 not searched");
    assert_eq!(sh.sent_count_label.text(), "1 sent (1 queued)");
    assert_eq!(sh.run_eta_label.text(), IDLE_ETA);
    // Reviewing a row adds the "to review" figure, which is quiet until something is reviewed.
    {
        let mut q = f.w.s.queue.borrow_mut();
        let id = q.id_at(0).unwrap();
        q.entry_mut(id).unwrap().reviewed = true;
    }
    f.w.list_changed();
    assert_eq!(
        sh.sent_count_label.text(),
        "1 sent (1 queued) \u{b7} 1 to review"
    );
    // An empty list: nothing queued, nothing sent.
    *f.w.s.queue.borrow_mut() =
        hatate_core::queue::QueueModel::new(Vec::new(), f.w.s.settings.clone());
    f.w.list_changed();
    assert_eq!(sh.run_progress_label.text(), IDLE_PROGRESS);
    assert_eq!(sh.sent_count_label.text(), "");
    f.close();
}

fn add_files_hashes_and_dedupes(app: &adw::Application) {
    let f = fx(app);
    let imgs = make_images(&f.dir.path().join("pics"), &["a.png", "b.jpg", "c.webp"]);
    f.script.push(Answer::Files(imgs.clone()));
    f.activate("add-files");
    assert!(
        pump_until(10_000, || f.len() == 3),
        "the three files were added"
    );
    assert_eq!(f.say(), "Added 3 file(s), 3 total");
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("rows")
    );
    assert_eq!(f.w.s.shell.root.glyph(), "\u{529B}");
    assert!(
        matches!(&f.script.asked.borrow()[0], Asked::OpenFiles { title, .. } if title == "Add images")
    );
    for e in f.w.s.queue.borrow().entries() {
        assert!(
            e.hydrus_hash.as_deref().is_some_and(|h| h.len() == 64),
            "hash filled"
        );
        assert_eq!(e.status, MatchStatus::NotSearched);
    }
    // Everything already there: a box says so, and nothing is added.
    f.script.push(Answer::Files(imgs.clone()));
    f.activate("add-files");
    assert!(pump_until(10_000, || f.script.count_shown() == 1));
    let shown = f.script.last_shown();
    assert_eq!(
        (
            shown.kind,
            shown.title.as_str(),
            shown.text.as_str(),
            shown.buttons
        ),
        (
            Kind::Information,
            "Add Files",
            "All 3 file(s) are already in the list.",
            Buttons::Ok
        )
    );
    assert_eq!(f.len(), 3);
    // One new, two known: the status says so.
    let more = make_images(&f.dir.path().join("pics"), &["d.gif"]);
    let mut batch = imgs[..2].to_vec();
    batch.extend(more);
    f.script.push(Answer::Files(batch));
    f.activate("add-files");
    assert!(pump_until(10_000, || f.len() == 4));
    assert_eq!(f.say(), "Added 1 file(s), 4 total (2 duplicate(s) skipped)");
    // Same content under another name is a duplicate too (the hash round).
    let copy = f.dir.path().join("pics").join("copy-of-a.png");
    std::fs::copy(&imgs[0], &copy).unwrap();
    f.script.push(Answer::Files(vec![copy]));
    f.activate("add-files");
    assert!(pump_until(10_000, || f.script.count_shown() == 2));
    assert_eq!(
        f.script.last_shown().text,
        "All 1 file(s) are already in the list."
    );
    // Nothing chosen: no job, no change.
    f.script.push(Answer::Files(vec![]));
    f.activate("add-files");
    pump_for(50);
    assert_eq!(f.len(), 4);
    f.close();
}

fn add_folder_scans_and_drop_filters(app: &adw::Application) {
    let f = fx(app);
    let folder = f.dir.path().join("lib");
    make_images(&folder.join("sub"), &["x.png", "y.mp4"]);
    make_images(&folder, &["top.jpg", "notes.txt"]);
    f.script.push(Answer::Path(Some(folder.clone())));
    f.activate("add-folder");
    assert!(
        pump_until(10_000, || f.len() == 3),
        "png, mp4 and jpg, not the .txt"
    );
    assert!(
        matches!(&f.script.asked.borrow()[0], Asked::Folder { title } if title == "Add folder")
    );
    // Cancelled chooser: nothing.
    f.script.push(Answer::Path(None));
    f.activate("add-folder");
    pump_for(50);
    assert_eq!(f.len(), 3);
    // A drop keeps folders (scanned) and files with an image extension.
    let other = f.dir.path().join("drop");
    let imgs = make_images(&other, &["p.png", "q.txt"]);
    f.w.drop_paths(imgs);
    assert!(pump_until(10_000, || f.len() == 4));
    f.close();
}

fn a_second_add_waits_and_the_window_keeps_working(app: &adw::Application) {
    let f = fx(app);
    // A file that never answers: a named pipe with no writer yet.
    let fifo = f.dir.path().join("stalled.png");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    f.script.push(Answer::Files(vec![fifo.clone()]));
    f.activate("add-files");
    assert!(
        pump_until(2_000, || f.say().starts_with("Hashing")),
        "{}",
        f.say()
    );
    assert_eq!(f.say(), "Hashing 1 file(s)\u{2026}");
    // Another add is refused while this one runs.
    let other = make_images(&f.dir.path().join("o"), &["o.png"]);
    f.script.push(Answer::Files(other));
    f.activate("add-files");
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Add Files");
    assert_eq!(
        shown.text,
        "Please wait for the current batch to finish being added."
    );
    // And the window still handles actions: a theme switch, the log viewer.
    f.activate("view-logs");
    assert!(f.w.s.log_viewer.borrow().is_some());
    pump_for(100);
    // Release the pipe: the hash completes and the file is added.
    let writer = std::thread::spawn({
        let fifo = fifo.clone();
        move || std::fs::write(fifo, b"released").unwrap()
    });
    assert!(pump_until(10_000, || f.len() == 1));
    writer.join().unwrap();
    assert_eq!(f.say(), "Added 1 file(s), 1 total");
    f.close();
}

fn seeded(n: usize, status: MatchStatus) -> Vec<hatate_core::model::Entry> {
    (0..n)
        .map(|i| entry(&format!("/lib/img{i}.png"), status))
        .collect()
}

fn restore_clean_session(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 3);
    assert_eq!(f.say(), "Restored 3 image(s) from your last session");
    assert!(!f.w.s.run_banner.root.is_visible());
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("rows")
    );
    f.close();
    // `restore_session_on_start` off: nothing is read.
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            tweak: Some(Box::new(|s| s.restore_session_on_start = false)),
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 0);
    f.close();
}

fn restore_after_a_crash_shows_the_banner(app: &adw::Application) {
    let mut rows = seeded(4, MatchStatus::Good);
    rows[0].sent_to_hydrus = true;
    rows[0].hydrus_import_confirmed = true;
    rows[1].status = MatchStatus::Searching;
    rows[2].status = MatchStatus::NotSearched;
    let f = Fx::new(
        app,
        Opts {
            unclean: true,
            seed: rows,
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 4);
    assert!(f.w.s.run_banner.root.is_visible());
    let body = f.w.s.run_banner.parts.body.text().to_string();
    assert_eq!(
        body,
        "Hatate did not exit cleanly during this run. Everything up to the last autosave is safe. \
         1 of 4 images are already sent to Hydrus, 1 is decided and waiting for your review, and 2 \
         were never reached."
    );
    // The mid-search row went back to unsearched and is flagged.
    let q = f.w.s.queue.borrow();
    assert_eq!(q.entries()[1].status, MatchStatus::NotSearched);
    assert!(q.entries()[1].interrupted_mid_search);
    drop(q);
    // The banner replaces the status-bar line rather than joining it.
    assert_eq!(f.say(), "Ready");
    assert!(f.w.s.shell.run_note_label.text().starts_with("stopped "));
    f.close();
}

fn nothing_survived_variant(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            unclean: true,
            empty_session_file: true,
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 0);
    assert_eq!(
        f.say(),
        "Run interrupted \u{2014} nothing survived from the last session"
    );
    assert_eq!(f.w.s.shell.run_note_label.text(), RUN_FAILED_NOTE);
    assert!(f.w.s.nothing_survived.borrow().is_some());
    assert!(!f.w.s.run_banner.root.is_visible());
    // Its button goes back to the ordinary drop zone and clears the note.
    let btn =
        f.w.s
            .nothing_survived
            .borrow()
            .as_ref()
            .unwrap()
            .new_search
            .clone();
    btn.emit_clicked();
    assert!(f.w.s.nothing_survived.borrow().is_none());
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("empty")
    );
    assert_eq!(f.w.s.shell.run_note_label.text(), "");
    f.close();
    // A crash with no session file at all shows nothing: "never saved anything".
    let f = Fx::new(
        app,
        Opts {
            unclean: true,
            ..Opts::default()
        },
    );
    assert!(f.w.s.nothing_survived.borrow().is_none());
    f.close();
}

fn missing_files_dialog(app: &adw::Application) {
    let missing: hatate_gtk::window::MissingFiles =
        Arc::new(|paths, _| paths.iter().take(2).cloned().collect::<HashSet<String>>());
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(5, MatchStatus::Good),
            missing: Some(missing),
            ..Opts::default()
        },
    );
    assert!(pump_until(5_000, || f.script.count_shown() == 1));
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.kind, shown.title.as_str(), shown.buttons),
        (Kind::Information, "Missing files", Buttons::Ok)
    );
    assert!(shown.text.starts_with("2 of 5 image(s) in this session are no longer on disk - most likely deleted from Hydrus since the session was saved.\n\nThey're still listed, marked \"missing\" in the File column"));
    assert_eq!(
        f.say(),
        "2 image(s) in this list are no longer on disk - see the File column"
    );
    f.close();
    // Nothing missing: nothing said.
    let none: hatate_gtk::window::MissingFiles = Arc::new(|_, _| HashSet::new());
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            missing: Some(none),
            ..Opts::default()
        },
    );
    pump_for(200);
    assert_eq!(f.script.count_shown(), 0);
    f.close();
}

fn save_session_as(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let target = f.dir.path().join("kept");
    f.script.push(Answer::Path(Some(target.clone())));
    f.activate("save-session-as");
    let saved = f.dir.path().join("kept.json");
    assert!(saved.exists(), "the .json suffix is added");
    assert_eq!(named::entry_count(&saved), Some(3));
    assert_eq!(f.say(), "Saved 3 image(s) to kept.json");
    let asked = f.script.asked.borrow().last().cloned().unwrap();
    let Asked::SaveFile {
        title,
        default_name,
        filter,
    } = asked
    else {
        panic!("not a save chooser")
    };
    assert_eq!(title, "Save Session As");
    assert!(
        default_name.contains("hatate-session-") && default_name.ends_with(".json"),
        "{default_name}"
    );
    assert_eq!(filter, "Session files (*.json);;All files (*)");
    // A cancelled chooser saves nothing; an unwritable path warns with Python's words.
    f.script.push(Answer::Path(None));
    f.activate("save-session-as");
    let bad = PathBuf::from("/nonexistent-dir-for-test/x.json");
    f.script.push(Answer::Path(Some(bad.clone())));
    f.activate("save-session-as");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.kind, shown.title.as_str()),
        (Kind::Warning, "Save Session")
    );
    assert_eq!(
        shown.text,
        format!(
            "Could not write the session to:\n{}\n\nSee Help > View Logs for details.",
            bad.display()
        )
    );
    f.close();
    // An empty list says so and asks nothing.
    let f = fx(app);
    f.activate("save-session-as");
    assert_eq!(
        f.script.last_shown().text,
        "There's nothing in the list to save."
    );
    assert!(f.script.asked.borrow().is_empty());
    f.close();
}

fn open_session(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let theirs = f.dir.path().join("theirs.json");
    let rows = seeded(5, MatchStatus::Poor);
    named::save(&theirs, &rows, NOW).unwrap();
    // Replacing a non-empty list asks first, and the default answer is No.
    f.script.push(Answer::Path(Some(theirs.clone())));
    f.script.push(Answer::Msg(Response::No));
    f.activate("open-session");
    let q = f.script.last_shown();
    assert_eq!(
        (q.kind, q.title.as_str(), q.buttons, q.default),
        (Kind::Question, "Open Session", Buttons::YesNo, Response::No)
    );
    assert_eq!(
        q.text,
        "Replace the current list of 2 image(s) with 5 image(s) from theirs.json?\n\nAnything unsaved in the current list will be lost. Your files and anything already sent to Hydrus are untouched."
    );
    assert_eq!(f.len(), 2, "No keeps the list");
    f.script.push(Answer::Path(Some(theirs.clone())));
    f.script.push(Answer::Msg(Response::Yes));
    f.activate("open-session");
    assert_eq!(f.len(), 5);
    assert_eq!(f.say(), "Opened theirs.json - 5 image(s)");
    // Not a session: a warning with the path.
    let junk = f.dir.path().join("junk.json");
    std::fs::write(&junk, "[1, 2]").unwrap();
    f.script.push(Answer::Path(Some(junk.clone())));
    f.activate("open-session");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.kind, shown.title.as_str()),
        (Kind::Warning, "Open Session")
    );
    assert_eq!(
        shown.text,
        format!(
            "That doesn't look like a session file this build can read:\n{}\n\nSee Help > View Logs for details.",
            junk.display()
        )
    );
    // A readable session with no entries.
    let empty = f.dir.path().join("empty.json");
    named::save(&empty, &[], NOW).unwrap();
    f.script.push(Answer::Path(Some(empty.clone())));
    f.script.push(Answer::Msg(Response::Yes));
    f.activate("open-session");
    assert_eq!(
        f.script.last_shown().text,
        format!("No usable entries were found in:\n{}", empty.display())
    );
    assert_eq!(f.len(), 5);
    // While a search runs it refuses.
    f.w.s.queue.borrow_mut().set_search_running(true);
    f.activate("open-session");
    assert_eq!(
        f.script.last_shown().text,
        "A search is running. Stop it before opening a different session."
    );
    f.close();
}

fn clear_session_matches_the_empty_list_quirk(app: &adw::Application) {
    // Python's `clear_session()` runs before the question when the list is empty, so a No still
    // leaves the saved session discarded. The list is empty here because restore is off.
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            tweak: Some(Box::new(|s| s.restore_session_on_start = false)),
            ..Opts::default()
        },
    );
    assert_eq!(f.saved_count(), 3);
    f.script.push(Answer::Msg(Response::No));
    f.activate("clear-session");
    let q = f.script.last_shown();
    assert_eq!(
        (q.title.as_str(), q.default),
        ("Clear Saved Session", Response::No)
    );
    assert_eq!(
        q.text,
        "Empty the current list and discard the saved session?\n\nThis only affects this app's list - your files and anything already sent to Hydrus are untouched."
    );
    assert_eq!(
        f.saved_count(),
        0,
        "the session was discarded before the question"
    );
    f.close();
    // With a list, No changes nothing and Yes clears both.
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.script.push(Answer::Msg(Response::No));
    f.activate("clear-session");
    assert_eq!((f.len(), f.saved_count()), (3, 3));
    f.script.push(Answer::Msg(Response::Yes));
    f.activate("clear-session");
    assert_eq!((f.len(), f.saved_count()), (0, 0));
    assert_eq!(f.say(), "Cleared the list and the saved session");
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("empty")
    );
    f.close();
}

fn persisted_quota_pause(app: &adw::Application) {
    let live = QuotaPause {
        paused_at: NOW - 3600.0,
        searched: 12,
        remaining: 1234,
        reset_at: hatate_core::timeutil::next_utc_midnight_iso(NOW - 2.0 * 86_400.0),
    };
    // The reset is two days behind the clock: the allowance has come back.
    let reset = live.reset_epoch().unwrap();
    let setup = move |paths: &hatate_core::paths::Paths| {
        QuotaPauseFile::new(paths).record(&live).unwrap();
    };
    let f = Fx::new(
        app,
        Opts {
            setup: Some(Box::new(setup)),
            ..Opts::default()
        },
    );
    assert!(reset < NOW);
    assert!(!f.w.s.quota_banner.root.is_visible());
    assert!(
        QuotaPauseFile::new(&f.paths).read().is_none(),
        "a stale pause is deleted"
    );
    f.close();

    let fresh = QuotaPause {
        paused_at: NOW - 600.0,
        searched: 5,
        remaining: 1234,
        reset_at: hatate_core::timeutil::next_utc_midnight_iso(NOW),
    };
    let fresh2 = fresh.clone();
    let setup = move |paths: &hatate_core::paths::Paths| {
        QuotaPauseFile::new(paths).record(&fresh2).unwrap()
    };
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            setup: Some(Box::new(setup)),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    assert!(f.w.s.quota_banner.root.is_visible());
    assert_eq!(f.script.count_shown(), 0, "a pause on disk raises no modal");
    let quota = hatate_gtk::text::describe_quota_pause(&fresh, NOW);
    // The restore status and the quota text join with a middle dot.
    assert_eq!(
        f.say(),
        format!("Restored 2 image(s) from your last session \u{b7} {quota}")
    );
    // The strip's note and estimate, as literal text (a constant compared with itself would pass
    // whatever the constant said), and the count on a paused strip: nothing waits to be searched.
    assert_eq!(
        f.w.s.shell.run_note_label.text(),
        "paused \u{2014} quota exhausted"
    );
    assert_eq!(f.w.s.shell.run_eta_label.text(), "ETA paused");
    assert_eq!(f.w.s.shell.run_progress_label.text(), "nothing queued");
    // Dismissing the pause takes the note and the estimate back off the strip.
    f.w.hide_quota_banner();
    assert_eq!(f.w.s.shell.run_note_label.text(), "");
    f.w.refresh_run_progress_label();
    assert_eq!(f.w.s.shell.run_eta_label.text(), "ETA \u{2014}");
    assert!(
        f.w.s
            .quota_banner
            .parts
            .body
            .text()
            .contains("Nothing was lost.")
    );
    f.close();
}

fn theme_action_saves_the_setting(app: &adw::Application) {
    let f = fx(app);
    let saved = |f: &Fx| hatate_core::config::Settings::load(&f.paths).settings.theme;
    assert_eq!(saved(&f), "system");
    f.w.set_theme("light");
    assert_eq!(saved(&f), "light");
    assert_eq!(f.w.s.scheme.get(), hatate_gtk::tokens::Scheme::Light);
    f.w.set_theme("dark");
    assert_eq!(
        (saved(&f), f.w.s.scheme.get()),
        ("dark".into(), hatate_gtk::tokens::Scheme::Dark)
    );
    // Not a mode: ignored, as `action_set_theme` does.
    f.w.set_theme("sepia");
    assert_eq!(saved(&f), "dark");
    // The menu action carries the state.
    gio::prelude::ActionGroupExt::activate_action(&f.w.win, "theme", Some(&"light".to_variant()));
    assert_eq!(saved(&f), "light");
    f.close();
}

fn close_saves_and_marks_a_clean_exit(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            setup: Some(Box::new(|p| {
                std::fs::write(p.crash_running_marker(), "1").unwrap();
            })),
            ..Opts::default()
        },
    );
    let imgs = make_images(&f.dir.path().join("p"), &["a.png", "b.png"]);
    f.script.push(Answer::Files(imgs));
    f.activate("add-files");
    assert!(pump_until(10_000, || f.len() == 2));
    assert_eq!(f.saved_count(), 0, "nothing saved yet");
    let started = hatate_core::crash_marker::CrashMarkers::begin(&f.paths, NOW);
    assert!(started.previous_run_unclean);
    *f.w.s.run_session.borrow_mut() = Some(started.session);
    f.w.close_teardown();
    assert!(f.w.s.closing.get());
    assert_eq!(f.saved_count(), 2, "the final save wrote the list");
    assert!(
        !f.paths.crash_running_marker().exists(),
        "the running marker is gone"
    );
    assert!(
        f.paths.last_clean_exit().exists(),
        "the clean exit is recorded"
    );
    // A second close does nothing more.
    f.w.close_teardown();
    f.close();
}

/// `closeEvent`'s `save_session(self.entries)` is a full write and is not skipped for an empty
/// list: a list emptied since the last save leaves an empty session, not the old rows (found by
/// the action differential, 13 scenarios).
fn close_writes_an_empty_list_over_the_old_rows(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 3);
    assert_eq!(f.saved_count(), 3, "the seeded session is on disk");
    *f.w.s.queue.borrow_mut() =
        hatate_core::queue::QueueModel::new(Vec::new(), f.w.s.settings.clone());
    f.w.list_changed();
    f.w.close_teardown();
    assert_eq!(f.saved_count(), 0, "the final save wrote the empty list");
    f.close();
}

fn autosave_writes_the_list_off_the_main_thread(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.w.s
        .queue
        .borrow_mut()
        .push(entry("/lib/extra.png", MatchStatus::NotSearched));
    assert_eq!(f.saved_count(), 2);
    f.w.autosave_now();
    assert!(
        pump_until(10_000, || f.saved_count() == 3),
        "the autosave landed"
    );
    // An empty list is never written over a session that has contents.
    f.w.s.queue.replace(hatate_core::queue::QueueModel::new(
        vec![],
        f.w.s.settings.clone(),
    ));
    f.w.autosave_now();
    pump_for(150);
    assert_eq!(f.saved_count(), 3);
    f.close();
}

fn log_viewer_and_open_log(app: &adw::Application) {
    let f = fx(app);
    f.activate("view-logs");
    let v = f.w.s.log_viewer.borrow().clone().expect("viewer");
    assert_eq!(v.window.title().as_deref(), Some("Logs"));
    assert!(v.path_label.text().starts_with("Log file: "));
    assert!(v.path_label.text().ends_with("hatate-rs.log"));
    assert_eq!(v.body(), "(no log entries yet)");
    assert_eq!(v.crash_log_btn.label().as_deref(), Some("View Crash Log"));
    v.toggle_crash_log();
    assert!(v.path_label.text().starts_with("Crash log: "));
    assert_eq!(v.body(), "(no crash log yet - nothing has crashed)");
    assert_eq!(v.crash_log_btn.label().as_deref(), Some("View App Log"));
    // The crash banner's button opens it pre-switched.
    f.w.open_crash_log();
    assert!(
        f.w.s
            .log_viewer
            .borrow()
            .as_ref()
            .unwrap()
            .showing_crash_log()
    );
    // Open Matched URLs Log: absent, then present.
    f.activate("open-log");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.title.as_str(), shown.text.as_str()),
        ("Matched URLs Log", "The log file doesn't exist yet.")
    );
    let log = PathBuf::from(f.w.s.settings.lock().unwrap().log_file_path.clone());
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "x").unwrap();
    f.activate("open-log");
    assert_eq!(*f.opened.borrow(), vec![log]);
    f.close();
}

fn fonts_cover_the_status_glyphs_and_kanji(app: &adw::Application) {
    let f = fx(app);
    let ctx = f.w.win.pango_context();
    let has = |family: &str, ch: char| {
        let desc = gtk4::pango::FontDescription::from_string(&format!("{family} 14"));
        let font = ctx
            .load_font(&desc)
            .unwrap_or_else(|| panic!("{family} did not load"));
        font.describe()
            .family()
            .is_some_and(|f| f.as_str().eq_ignore_ascii_case(family))
            && font.has_char(ch)
    };
    for ch in hatate_gtk::fonts::STATUS_GLYPHS {
        assert!(has("Ryoku Status", ch), "Ryoku Status lacks {ch:?}");
    }
    for ch in hatate_gtk::fonts::KANJI_GLYPHS {
        assert!(has("Ryoku Kanji", ch), "Ryoku Kanji lacks {ch:?}");
    }
    for family in ["Space Grotesk", "JetBrains Mono", "Source Serif 4"] {
        assert!(has(family, 'A'), "{family} did not load");
    }
    // `LoadOptions` is part of the public restore contract the scenarios rely on.
    let _ = (LoadOptions::at(NOW), SessionStore::new(&f.paths));
    f.close();
}

/// The content's own minimum, with no `set_size_request` floor: width, then height at that width.
fn content_min(f: &Fx) -> (i32, i32) {
    let content =
        libadwaita::prelude::AdwApplicationWindowExt::content(&f.w.win).expect("window content");
    let (w, _, _, _) = content.measure(gtk4::Orientation::Horizontal, -1);
    let (h, _, _, _) = content.measure(gtk4::Orientation::Vertical, w);
    (w, h)
}

/// Design gate 2.5: the stated minimum fits every state of the shell's surfaces, dark and light,
/// and the window really is that size when it is given it (outer rect recorded).
fn minimum_size_fits_every_state_in_both_schemes(app: &adw::Application) {
    use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile};
    use hatate_gtk::window::{MIN_HEIGHT, MIN_WIDTH};
    let mut worst = (0, 0);
    for scheme in ["dark", "light"] {
        let tweak = move |s: &mut hatate_core::config::Settings| s.theme = scheme.to_owned();
        type Build = fn(&adw::Application, &str) -> Fx;
        let states: [(&str, Build, &str); 8] = [
            (
                "queue-empty",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "queue-rows",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            seed: seeded(5, MatchStatus::Good),
                            missing: Some(Arc::new(|_, _| HashSet::new())),
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "queue-crashed-banner",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            unclean: true,
                            seed: seeded(5, MatchStatus::Good),
                            missing: Some(Arc::new(|_, _| HashSet::new())),
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "queue-nothing-survived",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            unclean: true,
                            empty_session_file: true,
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "queue-quota-banner",
                |app, scheme| {
                    let fresh = QuotaPause {
                        paused_at: NOW - 600.0,
                        searched: 5,
                        remaining: 1234,
                        reset_at: hatate_core::timeutil::next_utc_midnight_iso(NOW),
                    };
                    Fx::new(
                        app,
                        Opts {
                            seed: seeded(5, MatchStatus::Good),
                            missing: Some(Arc::new(|_, _| HashSet::new())),
                            setup: Some(Box::new(move |p| {
                                QuotaPauseFile::new(p).record(&fresh).unwrap()
                            })),
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "review",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "review",
            ),
            (
                "queue-failed-banner",
                |app, scheme| {
                    let f = Fx::new(
                        app,
                        Opts {
                            seed: seeded(5, MatchStatus::NotSearched),
                            missing: Some(Arc::new(|_, _| HashSet::new())),
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    );
                    f.w.show_failed_banner(
                        "Hatate could not make its temporary folder for automatic imports. Check \
                         that the cache folder exists and can be written to.",
                    );
                    f
                },
                "queue",
            ),
            (
                "activity",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| {
                                    c.theme = s.clone();
                                    // Every extra on: the widest chip row the page can have.
                                    c.enable_tracemoe = true;
                                    c.enable_iqdb3d = true;
                                    c.enable_google_images = true;
                                    c.enable_google_lens = true;
                                    c.enable_yandex = true;
                                    c.enable_pawchive = true;
                                    c.secondary_engine_mode = "always".to_owned();
                                }
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "activity",
            ),
        ];
        let _ = tweak;
        for (name, build, mode) in states {
            let f = build(app, scheme);
            f.w.set_mode(mode);
            pump_for(150);
            let (w, h) = content_min(&f);
            let _ = h;
            println!("MINSIZE {name} {scheme} content_min_width={w}");
            // The window at the stated minimum: its outer rect, and what the content is given
            // inside it. Height depends on width (wrapped prose), so the need is measured at the
            // width the content is actually given.
            f.w.win.set_default_size(1, 1);
            pump_for(200);
            let content = libadwaita::prelude::AdwApplicationWindowExt::content(&f.w.win).unwrap();
            let (cw, ch) = (content.width(), content.height());
            let (_, need_h_at_cw, _, _) = content.measure(gtk4::Orientation::Vertical, cw);
            // What the toplevel asks for before it knows a width (GTK 4.14's FlowBox answers this
            // with its chips stacked in one column: the Activity page then wanted 909 px, DAN-2324).
            let (need_h_unconstrained, _, _, _) = content.measure(gtk4::Orientation::Vertical, -1);
            println!(
                "OUTER {name} {scheme} window={}x{} content={cw}x{ch} needs={w}x{need_h_at_cw} unconstrained_min_h={need_h_unconstrained}",
                f.w.win.width(),
                f.w.win.height()
            );
            assert!(
                f.w.win.width() <= MIN_WIDTH && f.w.win.height() <= MIN_HEIGHT,
                "{name} ({scheme}): the window had to grow past the stated minimum {MIN_WIDTH}x{MIN_HEIGHT}"
            );
            assert!(
                cw >= w && ch >= need_h_at_cw,
                "{name} ({scheme}) needs {w}x{need_h_at_cw} but the stated minimum gives it {cw}x{ch}"
            );
            // The content must also fit INSIDE the window: a child given more than the window
            // has runs off its bottom edge, and `ch >= need` above cannot see that.
            assert!(
                ch <= f.w.win.height(),
                "{name} ({scheme}): the content is allocated {ch} px in a window {} px tall; it runs off the bottom",
                f.w.win.height()
            );
            assert!(
                need_h_unconstrained <= MIN_HEIGHT,
                "{name} ({scheme}): asked for {need_h_unconstrained} px before a width is known; the stated minimum is {MIN_HEIGHT}"
            );
            worst = (worst.0.max(w), worst.1.max(need_h_at_cw));
            f.close();
        }
    }
    println!(
        "MINSIZE worst content_min={}x{} stated={MIN_WIDTH}x{MIN_HEIGHT}",
        worst.0, worst.1
    );
}

// ---- 6.e: running searches from the window ---------------------------------------------------

/// Settings every search scenario starts from: IQDB only, no pacing wait, no cache, nothing that
/// would reach a network.
fn search_tweak() -> Tweak {
    Box::new(|s: &mut hatate_core::config::Settings| {
        s.primary_engine = "iqdb".to_owned();
        s.secondary_engine_mode = "disabled".to_owned();
        s.enable_ascii2d = false;
        s.enable_tracemoe = false;
        s.enable_pawchive_index = false;
        s.use_search_cache = false;
        s.delay_min_seconds = 0.0;
        s.delay_max_seconds = 0.0;
        s.match_conditions.require_larger_than_local = false;
    })
}

/// `n` unsearched rows whose files exist (a 4 x 3 PNG each), under `dir`.
fn unsearched(dir: &std::path::Path, n: usize) -> Vec<hatate_core::model::Entry> {
    std::fs::create_dir_all(dir).unwrap();
    (0..n)
        .map(|i| {
            let p = dir.join(format!("img{i}.png"));
            std::fs::write(&p, common::search::PNG).unwrap();
            entry(&p.to_string_lossy(), MatchStatus::NotSearched)
        })
        .collect()
}

fn search_fx(app: &adw::Application, pics: &std::path::Path, n: usize) -> Fx {
    Fx::new(
        app,
        Opts {
            seed: unsearched(pics, n),
            tweak: Some(search_tweak()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    )
}

fn statuses(f: &Fx) -> Vec<MatchStatus> {
    f.w.s
        .queue
        .borrow()
        .entries()
        .iter()
        .map(|e| e.status)
        .collect()
}

fn start_button(f: &Fx) -> String {
    f.w.s
        .shell
        .search_toggle_btn
        .label()
        .unwrap_or_default()
        .to_string()
}

/// `action_start_search` over three rows: the strip, the status line, the button and the rows
/// follow the worker, and the list is the searched one when it ends.
fn start_search_runs_the_batch(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 3);
    for _ in 0..3 {
        f.search
            .engines
            .iqdb
            .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    }
    assert_eq!(start_button(&f), "\u{25B6} Start Search");
    assert_eq!(f.w.s.shell.run_progress_label.text(), "3 not searched");
    f.activate("toggle-search");
    // At once: the gauge is at 0 of 3, the strip is live, the button says Stop, and the rows are
    // still the list's own until the worker reports.
    assert_eq!(start_button(&f), "\u{25A0} Stop Search");
    assert!(f.w.s.run_active.get());
    assert!(f.w.s.queue.borrow().search_running());
    assert_eq!(f.w.s.run_progress.get(), (0, 3));
    assert_eq!(f.w.s.shell.run_progress_label.text(), "0/3 searched");
    assert_eq!(f.say(), "Searching 3 image(s)\u{2026}");
    assert_eq!(f.search.resets.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        pump_until(10_000, || !f.w.s.run_active.get()),
        "the run never finished: {}",
        f.say()
    );
    assert_eq!(statuses(&f), vec![MatchStatus::NotFound; 3]);
    assert_eq!(f.w.s.run_progress.get(), (3, 3));
    assert_eq!(f.w.s.shell.run_progress_label.text(), "3/3 searched");
    assert_eq!(f.say(), "Search finished");
    assert_eq!(start_button(&f), "\u{25B6} Start Search");
    assert!(f.w.s.shell.search_toggle_btn.is_sensitive());
    assert!(!f.w.s.queue.borrow().search_running());
    // The ETA is not a lie about a run that is over: the strip is back to its idle reading.
    assert_eq!(f.w.s.shell.run_eta_label.text(), IDLE_ETA);
    // The engine order the scripted engines saw: one IQDB call per row.
    assert_eq!(f.search.engines.calls(), vec!["iqdb"; 3]);
    f.close();
}

/// `test_starting_a_search_clears_it` (test_interrupted_banner.py): after a crash the strip says
/// "stopped 2h 14m ago"; starting a search forgets that, because it describes the run before.
fn starting_a_search_clears_the_stopped_note(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = Fx::new(
        app,
        Opts {
            unclean: true,
            seed: unsearched(pics.path(), 2),
            tweak: Some(search_tweak()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    assert!(
        f.w.s.shell.run_note_label.text().starts_with("stopped "),
        "{}",
        f.w.s.shell.run_note_label.text()
    );
    for _ in 0..2 {
        f.search
            .engines
            .iqdb
            .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    }
    f.activate("toggle-search");
    assert_eq!(f.w.s.shell.run_note_label.text(), "");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    f.close();
}

/// `action_start_search`'s two refusals.
fn start_search_with_nothing_to_do_or_one_running(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = Fx::new(
        app,
        Opts {
            seed: vec![entry(
                &pics.path().join("done.png").to_string_lossy(),
                MatchStatus::Good,
            )],
            tweak: Some(search_tweak()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.activate("toggle-search");
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Search");
    assert_eq!(shown.text, "No unsearched images in the list.");
    assert_eq!(shown.buttons, Buttons::Ok);
    assert!(!f.w.s.queue.borrow().search_running());
    f.close();

    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 2);
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::After(
            std::time::Duration::from_millis(400),
            Ok(Vec::new()),
        ));
    f.activate("toggle-search");
    assert!(f.w.s.queue.borrow().search_running());
    // A second Start while one runs (the menu cannot do it, a shortcut or the MCP tool can).
    f.w.action_start_search();
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.title.as_str(), shown.text.as_str()),
        ("Search", "A search is already running.")
    );
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    f.close();
}

/// `action_stop_search`: "Stopping after current image\u{2026}", the button off until the worker
/// is done, and the image under way is finished and kept.
fn stop_search_finishes_the_image_under_way(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 3);
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::After(
            std::time::Duration::from_millis(300),
            Ok(Vec::new()),
        ));
    f.activate("toggle-search");
    // The first image is in the engine's hands; Stop now.
    pump_for(50);
    f.activate("toggle-search");
    assert_eq!(f.say(), "Stopping after current image\u{2026}");
    assert!(!f.w.s.shell.search_toggle_btn.is_sensitive());
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(
        statuses(&f),
        vec![
            MatchStatus::NotFound,
            MatchStatus::NotSearched,
            MatchStatus::NotSearched
        ]
    );
    assert_eq!(f.say(), "Search finished");
    assert!(f.w.s.shell.search_toggle_btn.is_sensitive());
    assert_eq!(start_button(&f), "\u{25B6} Start Search");
    assert_eq!(f.w.s.shell.run_progress_label.text(), "1/3 searched");
    // Interrupted in-session shows no banner: the user just did it.
    assert!(!f.w.s.run_banner.root.is_visible());
    f.close();
}

/// A daily quota that runs out mid-batch: no modal, the banner with both actions, the pause on
/// disk and on the strip; "Continue with other engines" resumes without SauceNAO for this run
/// only.
fn a_quota_pause_shows_the_banner_and_continue_resumes(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = Fx::new(
        app,
        Opts {
            seed: unsearched(pics.path(), 3),
            tweak: Some(Box::new(|s: &mut hatate_core::config::Settings| {
                search_tweak()(s);
                s.primary_engine = "saucenao".to_owned();
                s.saucenao.pause_search_on_quota_exhausted = true;
                s.continue_without_saucenao_on_quota = false;
            })),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    for _ in 0..3 {
        f.search
            .engines
            .saucenao
            .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    }
    // The quota SauceNAO's last response reported: the strip's readout is "SauceNAO " (no colon)
    // plus the daily count, and its tooltip is the burst window (DAN-2326).
    *f.search.last_quota.lock().unwrap() = Some(SauceNaoQuota {
        short_remaining: Some(4),
        short_limit: Some(6),
        long_remaining: Some(198),
        long_limit: Some(200),
    });
    // The allowance is spent after the first request.
    f.search
        .quota_after_calls
        .store(1, std::sync::atomic::Ordering::SeqCst);
    f.activate("toggle-search");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    let readout = &f.w.s.shell.saucenao_quota_label;
    assert_eq!(readout.text(), "SauceNAO 2/200 used today");
    assert_eq!(
        readout.root.tooltip_text().as_deref(),
        Some("2/6 used in the current ~30s window")
    );
    // The banner's Review button works now (6.f): no reason to give for a disabled button.
    assert!(f.w.s.quota_banner.review.is_sensitive());
    assert_eq!(f.w.s.quota_banner.review.tooltip_text(), None);
    assert_eq!(
        statuses(&f),
        vec![
            MatchStatus::NotFound,
            MatchStatus::NotSearched,
            MatchStatus::NotSearched
        ]
    );
    assert_eq!(f.script.count_shown(), 0, "a quota pause raises no modal");
    assert!(f.w.s.quota_banner.root.is_visible());
    assert!(f.w.s.quota_paused.get());
    assert_eq!(
        f.w.s.shell.run_note_label.text(),
        "paused \u{2014} quota exhausted"
    );
    // `test_review_is_the_primary_and_continue_is_the_secondary`, the half a test can read: the
    // second action's words, and that the first is not offered while nothing has been decided
    // (the one row searched is a NotFound). Which is primary and their order are judged in the
    // shots.
    assert_eq!(
        f.w.s.quota_banner.keep_going.label().as_deref(),
        Some("Continue with other engines")
    );
    assert!(!f.w.s.quota_banner.review.is_visible());
    assert_eq!(f.w.s.shell.run_eta_label.text(), "ETA paused");
    let pause = QuotaPauseFile::new(&f.paths)
        .read()
        .expect("the pause is on disk");
    assert_eq!((pause.searched, pause.remaining), (1, 2));
    let texts = texts_under(f.w.s.quota_banner.root.upcast_ref());
    assert!(
        texts.iter().any(|t| t.contains("2 ")),
        "the banner counts what is left: {texts:?}"
    );
    // The run ending does not wipe the pause from the strip: the status line is the run's, the
    // note and the banner are the pause's.
    assert_eq!(f.say(), "Search finished");

    // Continue with other engines: SauceNAO is left out of this run, the saved setting is not
    // touched, the banner and the persisted pause go.
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.w.s.quota_banner.keep_going.emit_clicked();
    assert!(!f.w.s.quota_banner.root.is_visible());
    assert!(!f.w.s.quota_paused.get());
    assert!(QuotaPauseFile::new(&f.paths).read().is_none());
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(statuses(&f), vec![MatchStatus::NotFound; 3]);
    assert!(
        !f.w.s
            .settings
            .lock()
            .unwrap()
            .continue_without_saucenao_on_quota,
        "the saved setting is exactly as the user has it"
    );
    f.close();
}

/// `_engine_alert_forwarder` / `_on_engine_alert` / `engine_alerts.unsubscribe`: the channel is
/// open for the length of a run, an alert becomes a warning box and a status line, and an
/// unsubscribed listener stops hearing.
fn engine_alerts_reach_the_user_only_during_a_run(app: &adw::Application) {
    use hatate_core::engine_alerts::EngineAlert;
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 1);
    let alert = |key: &str| EngineAlert {
        key: key.to_owned(),
        engine: "Google Lens".to_owned(),
        title: "Google Lens needs a browser".to_owned(),
        body: "Lens cannot run: no browser was found.".to_owned(),
        remedy: "install chromium".to_owned(),
    };
    // Before a run nobody is listening.
    assert!(f.search.alerts.raise_alert(&alert("before")));
    pump_for(30);
    assert_eq!(f.script.count_shown(), 0);

    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::After(
            std::time::Duration::from_millis(250),
            Ok(Vec::new()),
        ));
    f.activate("toggle-search");
    pump_for(30);
    assert!(f.search.alerts.raise_alert(&alert("during")));
    assert!(pump_until(2_000, || f.script.count_shown() == 1));
    let shown = f.script.last_shown();
    assert_eq!(shown.kind, Kind::Warning);
    assert_eq!(shown.title, "Google Lens needs a browser");
    assert_eq!(
        shown.text,
        "Lens cannot run: no browser was found.\n\ninstall chromium"
    );
    assert_eq!(shown.buttons, Buttons::Ok);
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    // The listener comes off when the run ends (`test_an_unsubscribed_listener_stops_hearing`).
    assert!(f.search.alerts.raise_alert(&alert("after")));
    pump_for(30);
    assert_eq!(f.script.count_shown(), 1);
    f.close();
}

/// Nothing the engine does blocks the main loop: while the engine sits on a stalled answer the
/// window still handles actions and its timers.
fn the_window_keeps_working_while_an_engine_stalls(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 1);
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::After(
            std::time::Duration::from_millis(700),
            Ok(Vec::new()),
        ));
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let id = gtk4::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        gtk4::glib::ControlFlow::Continue
    });
    f.activate("toggle-search");
    pump_for(100);
    assert!(f.w.s.run_active.get(), "the engine is still stalled");
    let before = ticks.get();
    // An action handled while the search waits.
    f.w.set_theme("light");
    assert_eq!(f.w.s.settings.lock().unwrap().theme, "light");
    pump_for(200);
    assert!(
        ticks.get() >= before + 4,
        "the main loop kept turning: {before} -> {}",
        ticks.get()
    );
    assert!(f.w.s.run_active.get());
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    id.remove();
    f.close();
}

/// `_build_sites_menu`, `_on_site_toggled`, `_set_all_sites`: one check per site, the setting
/// follows each click and is saved, Select None / Select All set every check at once.
fn the_sites_menu_toggles_and_saves(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            tweak: Some(Box::new(|s: &mut hatate_core::config::Settings| {
                s.enabled_sites = vec!["Danbooru".to_owned(), "Pixiv".to_owned()];
            })),
            ..Opts::default()
        },
    );
    let all = menu::all_site_options();
    let state = |i: usize| -> bool {
        f.w.win
            .lookup_action(&menu::site_action(i))
            .and_then(|a| a.state())
            .and_then(|v| v.get::<bool>())
            .expect("a boolean site action")
    };
    for (i, site) in all.iter().enumerate() {
        assert_eq!(state(i), site == "Danbooru" || site == "Pixiv", "{site}");
    }
    let danbooru = all.iter().position(|s| s == "Danbooru").unwrap();
    let gelbooru = all.iter().position(|s| s == "Gelbooru").unwrap();
    // Clicking a site turns it off, and the saved config has it so.
    f.activate(&menu::site_action(danbooru));
    assert!(!state(danbooru));
    let saved = |paths: &hatate_core::paths::Paths| {
        hatate_core::config::Settings::load(paths)
            .settings
            .enabled_sites
    };
    assert_eq!(saved(&f.paths), vec!["Pixiv".to_owned()]);
    // And on: the list is in ALL_SITE_OPTIONS order, not click order.
    f.activate(&menu::site_action(gelbooru));
    f.activate(&menu::site_action(danbooru));
    let want: Vec<String> = all
        .iter()
        .filter(|s| ["Danbooru", "Gelbooru", "Pixiv"].contains(&s.as_str()))
        .cloned()
        .collect();
    assert_eq!(saved(&f.paths), want);
    f.activate("sites-select-none");
    assert!((0..all.len()).all(|i| !state(i)));
    assert!(saved(&f.paths).is_empty());
    f.activate("sites-select-all");
    assert!((0..all.len()).all(state));
    assert_eq!(saved(&f.paths), all);
    f.close();
}

/// `_research_rows_with_engine` and `_with_opposite_engine`: the named engine runs whatever the
/// configured order says, with the cache bypassed and the saved row overwritten.
fn re_search_uses_the_engine_it_was_asked_for(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let mut rows = unsearched(pics.path(), 2);
    rows[0].status = MatchStatus::NotFound;
    let f = Fx::new(
        app,
        Opts {
            seed: rows,
            tweak: Some(search_tweak()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let ids: Vec<_> = {
        let q = f.w.s.queue.borrow();
        (0..2).map(|r| q.id_at(r).unwrap()).collect()
    };
    f.search
        .engines
        .saucenao
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.w.run_menu_action(Action::ResearchWithEngine("saucenao"), &ids[..1]);
    assert_eq!(f.say(), "Searching (SauceNAO only) 1 image(s)\u{2026}");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(f.search.engines.calls(), vec!["saucenao"]);
    // Primary is IQDB, so a row with no match flips to SauceNAO.
    f.search
        .engines
        .saucenao
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.w.run_menu_action(Action::ResearchWithOppositeEngine, &ids[1..]);
    assert_eq!(f.say(), "Searching (opposite engine) 1 image(s)\u{2026}");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(f.search.engines.calls(), vec!["saucenao", "saucenao"]);
    // Plain re-search: the configured engine, "Re-searching".
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.w.run_menu_action(Action::Research, &ids[..1]);
    assert_eq!(f.say(), "Re-searching 1 image(s)\u{2026}");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(f.search.engines.calls()[2], "iqdb");
    f.close();
}

/// `Ending::Failed`: the worker dies on a staging directory that cannot be made. The window shows
/// a failed run (banner, strip note, counts), the message holds no path and no OS text, nothing
/// crashed, the rows stay as they were, and Dismiss clears it.
fn a_failed_run_is_shown_as_failed_not_as_a_crash(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = Fx::new(
        app,
        Opts {
            seed: unsearched(pics.path(), 2),
            tweak: Some(Box::new(|s: &mut hatate_core::config::Settings| {
                search_tweak()(s);
                s.auto_import_enabled = true;
                s.auto_import_method = "download_send".to_owned();
                s.hydrus.access_key = "synthetic-key".into();
            })),
            setup: Some(Box::new(|paths: &hatate_core::paths::Paths| {
                // A file where the staging directory has to be.
                let staging = paths.auto_import_staging_dir();
                std::fs::create_dir_all(staging.parent().unwrap()).unwrap();
                std::fs::write(&staging, b"in the way").unwrap();
            })),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.activate("toggle-search");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert!(f.w.s.failed_banner.root.is_visible());
    let said = texts_under(f.w.s.failed_banner.root.upcast_ref());
    let all = said.join(" | ");
    assert!(all.contains("Run failed"), "{all}");
    assert!(
        all.contains("Everything searched so far is saved, and 2 were not reached."),
        "{all}"
    );
    assert!(
        all.contains("Searched") && all.contains("Not searched"),
        "{all}"
    );
    let body = f.w.s.failed_banner.parts.body.text().to_string();
    for bad in ["/", "os error", "Not a directory"] {
        assert!(!body.contains(bad), "the banner body shows {bad:?}: {body}");
    }
    assert!(
        !all.contains(&*f.dir.path().to_string_lossy()),
        "no path: {all}"
    );
    assert_eq!(
        f.w.s.shell.run_note_label.text(),
        "last run failed \u{2014} see log"
    );
    assert_eq!(statuses(&f), vec![MatchStatus::NotSearched; 2]);
    assert_eq!(
        f.script.count_shown(),
        0,
        "a failed run is not a modal crash"
    );
    assert!(!f.w.s.queue.borrow().search_running());
    f.w.s.failed_banner.dismiss.emit_clicked();
    assert!(!f.w.s.failed_banner.root.is_visible());
    assert_eq!(f.w.s.shell.run_note_label.text(), "");
    f.close();
}

/// A row with `n` matches whose URLs the scripted host answers by path.
fn row_with_matches(path: &str, urls: &[&str]) -> hatate_core::model::Entry {
    let mut e = entry(path, MatchStatus::Good);
    e.candidates = urls
        .iter()
        .map(|u| hatate_core::model::Candidate::new(*u))
        .collect();
    e
}

/// `_check_match_availability` / `_on_availability_finished` / `_remove_dead_candidates`: the
/// refusals, the all-clear text, the Yes/No offer with the dead list, and Yes dropping them.
fn the_availability_check_offers_to_drop_the_dead_matches(app: &adw::Application) {
    // No match: said so, nothing started.
    let f = Fx::new(
        app,
        Opts {
            seed: vec![entry("/pics/none.png", MatchStatus::NotFound)],
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let id = f.w.s.queue.borrow().id_at(0).unwrap();
    f.w.run_menu_action(Action::CheckAvailability, &[id]);
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Check Match Availability");
    assert_eq!(shown.text, "This image has no matches to check.");
    f.close();

    // Every match is `unreachable.invalid`-free: the pre-set verdicts stand in for a network the
    // test never touches; the check itself is `hatate_core`'s (tests/availability_https.rs). Here
    // the shell is driven with the answer the worker would hand back.
    let f = Fx::new(
        app,
        Opts {
            seed: vec![row_with_matches(
                "/pics/two.png",
                &["https://a.test/1", "https://a.test/2"],
            )],
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let id = f.w.s.queue.borrow().id_at(0).unwrap();
    let mut done = f.w.s.queue.borrow().entry(id).cloned().unwrap();
    done.candidates[0].remote_available = Some(false);
    done.candidates[1].remote_available = None;
    f.script.push(Answer::Msg(Response::Yes));
    f.w.on_availability_finished(
        id,
        done,
        hatate_core::availability::EntryCheck {
            gone: 1,
            checked: 2,
        },
    );
    let shown = f.script.last_shown();
    assert_eq!(shown.kind, Kind::Question);
    assert_eq!(shown.buttons, Buttons::YesNo);
    assert!(
        shown.text.starts_with(
            "1 of 2 match(es) are no longer available.\n\nRemove them from this image's match list?"
        ),
        "{}",
        shown.text
    );
    assert!(
        shown
            .text
            .contains("(1 other(s) couldn't be checked conclusively and will be kept.)")
    );
    assert!(shown.text.contains("a.test \u{2014} https://a.test/1"));
    assert_eq!(shown.default, Response::No);
    assert_eq!(f.say(), "Removed 1 unavailable match(es)");
    let urls: Vec<String> =
        f.w.s
            .queue
            .borrow()
            .entry(id)
            .unwrap()
            .candidates
            .iter()
            .map(|c| c.url.clone())
            .collect();
    assert_eq!(urls, ["https://a.test/2"]);

    // No: kept, and said so.
    let mut done = f.w.s.queue.borrow().entry(id).cloned().unwrap();
    done.candidates[0].remote_available = Some(false);
    f.script.push(Answer::Msg(Response::No));
    f.w.on_availability_finished(
        id,
        done,
        hatate_core::availability::EntryCheck {
            gone: 1,
            checked: 1,
        },
    );
    assert_eq!(f.say(), "Availability check finished: 1 gone (kept)");
    assert_eq!(f.w.s.queue.borrow().entry(id).unwrap().candidates.len(), 1);

    // Nothing gone: the all-clear.
    let mut done = f.w.s.queue.borrow().entry(id).cloned().unwrap();
    done.candidates[0].remote_available = Some(true);
    f.w.on_availability_finished(
        id,
        done,
        hatate_core::availability::EntryCheck {
            gone: 0,
            checked: 1,
        },
    );
    assert_eq!(
        f.script.last_shown().text,
        "All 1 match(es) still appear to be available."
    );
    assert_eq!(f.say(), "Availability check finished: nothing gone");

    // A window that is closing says nothing (`test_availability_finished_is_a_no_op_while_closing`).
    let shown_before = f.script.count_shown();
    f.w.s.closing.set(true);
    let mut done = f.w.s.queue.borrow().entry(id).cloned().unwrap();
    done.candidates[0].remote_available = Some(false);
    f.w.on_availability_finished(
        id,
        done,
        hatate_core::availability::EntryCheck {
            gone: 1,
            checked: 1,
        },
    );
    assert_eq!(f.script.count_shown(), shown_before);
    f.close();
}

/// `_check_rows_for_upscaling`: the worker runs the real heuristics over a synthetic PNG, each row
/// gets a verdict and its detail, and the summary says how many were flagged.
fn the_upscale_check_marks_rows_and_summarises(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 2);
    let ids: Vec<_> = {
        let q = f.w.s.queue.borrow();
        (0..2).map(|r| q.id_at(r).unwrap()).collect()
    };
    f.w.run_menu_action(Action::CheckUpscaling, &ids);
    assert_eq!(f.say(), "Checking 2 image(s) for upscaling\u{2026}");
    // Running: a second request is refused.
    f.w.run_menu_action(Action::CheckUpscaling, &ids);
    assert_eq!(f.script.last_shown().text, "A check is already running.");
    assert!(pump_until(20_000, || f.script.count_shown() >= 2));
    {
        let q = f.w.s.queue.borrow();
        for e in q.entries() {
            assert!(
                matches!(e.upscale_verdict.as_deref(), Some("flagged" | "clear")),
                "{:?}",
                e.upscale_verdict
            );
            assert!(
                e.upscale_check_detail
                    .as_deref()
                    .is_some_and(|d| !d.is_empty())
            );
            assert_eq!((e.local_width, e.local_height), (Some(4), Some(3)));
        }
    }
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Upscale Check Results");
    assert!(
        shown
            .text
            .contains(" of 2 image(s) show possible signs of upscaling.")
    );
    assert!(shown.text.contains("img0.png:\n"));
    assert!(f.say().starts_with("Upscale check finished: "));
    f.close();
}

/// A probe that answers from a script after a pause, so a test can watch the window meanwhile.
struct ScriptedProbe {
    results: Vec<hatate_gtk::parser_health::ProbeResult>,
    pause: std::time::Duration,
}

impl hatate_gtk::parser_health::ProbeRunner for ScriptedProbe {
    fn run(
        &self,
        _timeout: f64,
        on_progress: &dyn Fn(usize, usize, &str),
    ) -> Vec<hatate_gtk::parser_health::ProbeResult> {
        for (i, r) in self.results.iter().enumerate() {
            on_progress(i, self.results.len(), &r.site);
            std::thread::sleep(self.pause);
        }
        self.results.clone()
    }
}

/// `action_parser_health`, `ParserHealthDialog`, `_warn_about_broken_parsers`.
fn parser_health_dialog_and_warning(app: &adw::Application) {
    use hatate_gtk::parser_health::ProbeResult;
    let probe = |site: &str, ok: bool, problem: Option<&str>| ProbeResult {
        site: site.to_owned(),
        ok,
        tags: 7,
        width: Some(800),
        height: Some(600),
        has_preview: true,
        has_file_url: true,
        problem: problem.map(str::to_owned),
        restricted: None,
    };
    let f = Fx::new_with(app, Opts::default(), |env| {
        env.probe = Some(Arc::new(ScriptedProbe {
            results: vec![
                probe("danbooru", true, None),
                probe("e621", false, Some("fetched, but produced no tags")),
            ],
            pause: std::time::Duration::from_millis(150),
        }));
    });
    assert!(f.w.win.lookup_action("parser-health").unwrap().is_enabled());
    f.activate("parser-health");
    let dialog =
        f.w.s
            .parser_health_dialog
            .borrow()
            .clone()
            .expect("the dialog is up");
    let cells = dialog.texts();
    assert_eq!(
        cells[0],
        ["Site", "This session", "Live check", "Last problem"]
    );
    assert_eq!(cells.len(), 1 + 13, "every site, before anything is known");
    assert!(
        cells[1..]
            .iter()
            .all(|r| r[1] == "not used yet" && r[2] == "\u{2014}")
    );

    // The live check runs off the main thread: the window's timers keep turning while it waits,
    // the button is off, and the progress and the summary line follow.
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let id = gtk4::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        gtk4::glib::ControlFlow::Continue
    });
    dialog.check_button.emit_clicked();
    assert!(!dialog.check_button.is_sensitive());
    assert!(dialog.progress.is_visible());
    assert_eq!(dialog.status.text(), "Checking\u{2026}");
    pump_for(120);
    assert!(ticks.get() >= 3, "the loop kept turning: {}", ticks.get());
    assert!(pump_until(5_000, || dialog.check_button.is_sensitive()));
    id.remove();
    assert!(!dialog.progress.is_visible());
    assert!(
        dialog
            .status
            .text()
            .starts_with("1 of 2 sites have a problem: e621. A site being down")
    );
    let rows = dialog.texts();
    let e621 = rows.iter().find(|r| r[0] == "e621").unwrap();
    assert_eq!(e621[2], "fetched, but produced no tags");
    assert_eq!(e621[3], "fetched, but produced no tags");
    let dan = rows.iter().find(|r| r[0] == "danbooru").unwrap();
    assert_eq!(dan[2], "7 tags, 800x600, preview, file");
    dialog.window.close();

    // `_warn_about_broken_parsers`: once per site, only after three empties and no success.
    let backend = f.w.s.search.backend_if_built().unwrap();
    let health = &backend.engine.booru.health;
    for _ in 0..2 {
        health.record_empty("rule34", "https://rule34.xxx/p/1");
    }
    f.w.warn_about_broken_parsers();
    assert_eq!(f.say(), "Ready", "two empties are not yet a fault");
    health.record_empty("rule34", "https://rule34.xxx/p/1");
    f.w.warn_about_broken_parsers();
    assert_eq!(
        f.say(),
        "rule34: no tags in 3 attempts - see Help > Parser Health"
    );
    f.w.say("Ready");
    f.w.warn_about_broken_parsers();
    assert_eq!(f.say(), "Ready", "reported once");
    f.close();

    // Without a live check the button says so and stays off.
    let f = fx(app);
    f.activate("parser-health");
    let dialog = f.w.s.parser_health_dialog.borrow().clone().unwrap();
    pump_for(150);
    // Opening it must not leave a site name looking picked: no label holds a selection, and the
    // first focus is on Close (DAN-2297 item A).
    let mut labels = Vec::new();
    let mut stack = vec![dialog.window.child().unwrap()];
    while let Some(w) = stack.pop() {
        if let Some(l) = w.downcast_ref::<gtk4::Label>() {
            labels.push(l.clone());
        }
        let mut c = w.first_child();
        while let Some(x) = c {
            c = x.next_sibling();
            stack.push(x);
        }
    }
    assert!(
        labels.iter().any(|l| l.is_selectable()),
        "the site names stay selectable so they can be copied"
    );
    assert!(
        labels.iter().all(|l| l.selection_bounds().is_none()),
        "a site name opened already selected"
    );
    let focus =
        gtk4::prelude::GtkWindowExt::focus(&dialog.window).expect("the dialog has a first focus");
    assert_eq!(
        focus
            .downcast_ref::<gtk4::Button>()
            .and_then(|b| b.label())
            .as_deref(),
        Some("Close"),
        "first focus is Close"
    );
    assert!(!dialog.check_button.is_sensitive());
    assert_eq!(
        dialog.texts()[0],
        ["Site", "This session", "Last problem"],
        "no Live check column when no live check runs"
    );
    assert_eq!(
        dialog.check_button.tooltip_text().unwrap(),
        "The live check is not part of this build yet."
    );
    // And the same words as a caption, so the reason is on screen without a hover.
    assert!(
        texts_under(dialog.window.child().unwrap().upcast_ref())
            .iter()
            .any(|t| t == "The live check is not part of this build yet.")
    );
    dialog.window.close();
    f.close();
}

/// A log source whose text the test changes.
struct MovingLog(std::cell::RefCell<String>);

impl hatate_gtk::logview::LogSource for MovingLog {
    fn log_path(&self) -> PathBuf {
        PathBuf::from("/synthetic/logs/hatate-rs.log")
    }
    fn recent_log(&self) -> String {
        self.0.borrow().clone()
    }
    fn crash_log_path(&self) -> PathBuf {
        PathBuf::from("/synthetic/logs/crash.log")
    }
    fn recent_crash_log(&self) -> String {
        String::new()
    }
}

/// `ActivityViewMixin`: the engine line-up read from the settings, the pipeline sentence, the log
/// pane that tails only while the page is on screen and the follow toggle is on.
fn activity_page_states_the_engines_and_tails_the_log(app: &adw::Application) {
    let log = Rc::new(MovingLog(std::cell::RefCell::new(String::new())));
    let l2 = log.clone();
    let f = Fx::new_with(
        app,
        Opts {
            tweak: Some(Box::new(|s: &mut hatate_core::config::Settings| {
                s.primary_engine = "iqdb".to_owned();
                s.secondary_engine_mode = "fallback".to_owned();
                s.enable_ascii2d = true;
                s.extras_only_as_fallback = true;
                s.delay_min_seconds = 45.0;
                s.delay_max_seconds = 75.0;
                s.use_search_cache = false;
            })),
            ..Opts::default()
        },
        move |env| env.logs = Some(l2),
    );
    let page = f.w.s.activity.clone();
    // Built with the window: the chips and the sentences are there before the page is shown.
    let chips: Vec<(String, bool)> = page.chip_texts().into_iter().map(|c| (c.0, c.1)).collect();
    assert_eq!(chips.len(), 10);
    assert_eq!(chips[0], ("IQDB \u{b7} primary".to_owned(), true));
    assert_eq!(chips[1], ("SauceNAO \u{b7} fallback".to_owned(), true));
    assert_eq!(chips[2], ("ascii2d \u{b7} fallback".to_owned(), true));
    assert_eq!(chips[3], ("trace.moe \u{b7} off".to_owned(), false));
    assert_eq!(page.chip_texts()[3].2, "Switched off in Settings > Engine.");
    assert_eq!(page.chip_texts()[0].2, "Runs on every image.");
    assert!(page.pipeline.text().contains("IQDB"));
    assert_eq!(
        page.summary.text(),
        "Paced at 45\u{2013}75s an image.  The search cache is off."
    );
    assert!(
        page.footer
            .text()
            .ends_with("/synthetic/logs/hatate-rs.log")
    );
    assert_eq!(page.log_text(), hatate_gtk::activity::NO_LOG_YET);

    // Off the page, nothing tails.
    assert!(page.timer.borrow().is_none());
    f.w.set_mode("activity");
    assert!(page.timer.borrow().is_some(), "on the page the log follows");
    *log.0.borrow_mut() = "first line\nsecond line".to_owned();
    f.w.tail_log();
    assert_eq!(page.log_text(), "first line\nsecond line");
    // The follow toggle off stops the tail; on, it reads at once and starts again.
    page.follow.set_active(false);
    assert!(page.timer.borrow().is_none());
    *log.0.borrow_mut() = "first line\nsecond line\nthird".to_owned();
    page.follow.set_active(true);
    assert_eq!(page.log_text(), "first line\nsecond line\nthird");
    assert!(page.timer.borrow().is_some());
    // The folder button hands the log's folder to the desktop.
    page.open_folder.emit_clicked();
    assert_eq!(
        f.opened.borrow().last().unwrap(),
        &PathBuf::from("/synthetic/logs")
    );
    // Leaving the page stops the tail; Show in Queue goes to Queue.
    page.show_in_queue.emit_clicked();
    assert_eq!(f.w.s.shell.current_mode(), "queue");
    assert!(page.timer.borrow().is_none());
    // Back on the page the engines are restated from the settings as they are now.
    f.w.s.settings.lock().unwrap().enable_tracemoe = true;
    f.w.set_mode("activity");
    assert_eq!(page.chip_texts()[3].0, "trace.moe \u{b7} fallback");
    // DAN-2297 ruling 2: the chips are not Tab stops. Walking Tab from the top of the page never
    // lands in the chip row (the tooltips stay for the pointer), and the walk gets past it to the
    // controls after it: marking only the FlowBox's wrappers unfocusable left the walk stuck on
    // Start Search.
    pump_for(150);
    f.w.win.set_focus_visible(true);
    let mut landed = Vec::new();
    for _ in 0..12 {
        f.w.win.child_focus(gtk4::DirectionType::TabForward);
        let w = gtk4::prelude::GtkWindowExt::focus(&f.w.win).expect("Tab leaves focus somewhere");
        assert!(
            !w.is_ancestor(page.chips.upcast_ref::<gtk4::Widget>())
                && &w != page.chips.upcast_ref::<gtk4::Widget>(),
            "a chip took the Tab focus"
        );
        landed.push(w);
    }
    assert!(
        landed.windows(2).any(|p| p[0] != p[1]),
        "Tab never moved off one widget"
    );
    assert!(
        landed.contains(&page.parser_health.clone().upcast::<gtk4::Widget>()),
        "Tab never reached the control after the chips"
    );
    f.close();
}

/// The first widget under `w` (itself included) with the CSS class `class`.
fn find_class(w: &gtk4::Widget, class: &str) -> Option<gtk4::Widget> {
    if w.has_css_class(class) {
        return Some(w.clone());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(found) = find_class(&child, class) {
            return Some(found);
        }
        c = child.next_sibling();
    }
    None
}

/// The first button under `w` whose label reads `text`.
fn find_button(w: &gtk4::Widget, text: &str) -> Option<gtk4::Button> {
    if let Some(b) = w.downcast_ref::<gtk4::Button>() {
        if b.label().as_deref() == Some(text) {
            return Some(b.clone());
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(found) = find_button(&child, text) {
            return Some(found);
        }
        c = child.next_sibling();
    }
    None
}

/// What the widget draws, `margin` px of room around its box included, as straight RGBA
/// (`width`, `height`, bytes); the widget's own box starts at `(margin, margin)`.
fn render(w: &gtk4::Widget, margin: i32) -> (usize, usize, Vec<u8>) {
    use gtk4::gdk;
    use gtk4::graphene::Rect;
    let paintable = gtk4::WidgetPaintable::new(Some(w));
    // The paintable's own size is the widget's border box; `Widget::width` is the content box inside
    // the CSS padding and border, and asking for that scales the picture down.
    let (bw, bh) = (
        gdk::prelude::PaintableExt::intrinsic_width(&paintable),
        gdk::prelude::PaintableExt::intrinsic_height(&paintable),
    );
    let snap = gtk4::Snapshot::new();
    gdk::prelude::PaintableExt::snapshot(&paintable, &snap, f64::from(bw), f64::from(bh));
    let node = snap.to_node().expect("snapshot");
    let renderer = w
        .native()
        .and_then(|n| n.renderer())
        .expect("a realized widget has a renderer");
    let (vw, vh) = (bw + 2 * margin, bh + 2 * margin);
    let view = Rect::new(-margin as f32, -margin as f32, vw as f32, vh as f32);
    let tex = renderer.render_texture(&node, Some(&view));
    let mut dl = gdk::TextureDownloader::new(&tex);
    dl.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = dl.download_bytes();
    assert_eq!(stride, vw as usize * 4);
    (vw as usize, vh as usize, bytes.to_vec())
}

fn px(img: &(usize, usize, Vec<u8>), x: usize, y: usize) -> [u8; 4] {
    let i = (y * img.0 + x) * 4;
    [img.2[i], img.2[i + 1], img.2[i + 2], img.2[i + 3]]
}

/// DAN-2242: what the design read measured on the shots, asserted on the widgets' own pixels, in
/// both schemes: the ghost kanji's box is 420 px tall (fix 1), the Queue frame is the raised fill
/// (fix 2), the plain `.card` draws no outline outside its box (fix 3), and a hollow control's
/// border is `ink-46` at rest and `ink-58` under the pointer (fix 4).
fn design_follow_ups_paint_as_specified(app: &adw::Application) {
    use hatate_gtk::tokens::{Palette, Scheme};
    for (name, scheme) in [("dark", Scheme::Dark), ("light", Scheme::Light)] {
        let f = Fx::new(
            app,
            Opts {
                tweak: Some(Box::new(move |c| c.theme = name.to_owned())),
                ..Opts::default()
            },
        );
        f.w.win.set_default_size(1100, 800);
        pump_for(400);
        let pal = Palette::of(scheme);
        let root = f.w.win.clone().upcast::<gtk4::Widget>();

        let ghost = find_class(&root, "ghost-kanji").expect("the ghost kanji");
        assert_eq!(
            ghost.height(),
            420,
            "{name}: the ghost label box is 420 px (line-height: 1 of a 420 px face), not the font's own 1.44 em"
        );

        let zone = find_class(&root, "drop-zone").expect("the empty-state frame");
        let img = render(&zone, 0);
        let want = pal.raised.hex();
        let at = |x, y| {
            let p = px(&img, x, y);
            format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2])
        };
        assert_eq!(
            at(16, img.1 - 16),
            want,
            "{name}: the frame interior is the raised fill"
        );
        assert_eq!(px(&img, 16, img.1 - 16)[3], 255, "{name}: and opaque");

        // The card: libadwaita's own `.card` draws a ring and a shadow outside its box, which a
        // paintable of the card alone does not hold. Read the margin around it from its parent.
        let card = find_class(&root, "card").expect("the Queue card");
        // The nearest ancestor with room around the card.
        let mut up = card.parent();
        let (parent, cx, cy) = loop {
            let p = up.expect("an ancestor with room around the card");
            let at = card
                .compute_point(&p, &gtk4::graphene::Point::new(0.0, 0.0))
                .expect("the card's place in its ancestor");
            if at.x() >= 20.0 && at.y() >= 20.0 {
                break (p, at.x() as usize, at.y() as usize);
            }
            up = p.parent();
        };
        let img = render(&parent, 0);
        let mid_y = cy + card.height() as usize / 2;
        let right = cx + card.width() as usize;
        assert!(right + 20 <= img.0, "{name}: room right of the card");
        let (ref_l, ref_r) = (px(&img, cx - 18, mid_y), px(&img, right + 18, mid_y));
        for d in 1..=8 {
            assert_eq!(
                px(&img, cx - d, mid_y),
                ref_l,
                "{name}: the page is plain {d}px left of the card (no outline or shadow)"
            );
            assert_eq!(
                px(&img, right + d - 1, mid_y),
                ref_r,
                "{name}: the page is plain {d}px right of the card"
            );
        }

        // A hollow control's border: the left edge column of the button, alpha only.
        let folder = find_button(&root, "Add folder\u{2026}").expect("Add folder");
        let edge_alpha = |b: &gtk4::Button| {
            let img = render(b.upcast_ref(), 0);
            f64::from(px(&img, 0, img.1 / 2)[3]) / 255.0
        };
        assert!(
            (edge_alpha(&folder) - pal.ink_46.3).abs() < 0.02,
            "{name}: Add folder's border is ink-46 ({}) at rest, got {}",
            pal.ink_46.3,
            edge_alpha(&folder)
        );
        // `.card` sets `border-radius: 0` for the framed variant: no 6.c page draws one, but
        // `widgets::card(_, true)` is the API the later pages build on, and libadwaita's own
        // `.card` would round its hairline to radius 12 (the corner pixel would then be clear).
        {
            let framed = hatate_gtk::widgets::card(Some("Framed"), true);
            let host = gtk4::Window::new();
            host.set_default_size(300, 120);
            host.set_child(Some(&framed));
            host.present();
            pump_for(300);
            let img = render(framed.upcast_ref(), 0);
            let corner = f64::from(px(&img, 0, 0)[3]) / 255.0;
            assert!(
                (corner - pal.ink_18.3).abs() < 0.02,
                "{name}: a framed card's corner is a square ink-18 hairline ({}), got alpha {corner}",
                pal.ink_18.3
            );
            host.close();
        }
        // D3 (DAN-2245): a disabled hollow control keeps its border at ink-46. libadwaita fades a
        // disabled button with `opacity` / `filter: opacity()`, which would take the edge to half of
        // this; the rule that cancels it (`opacity: 1; filter: none`) is what this reads.
        let query = find_button(&root, "Query Hydrus\u{2026}").expect("Query Hydrus");
        assert!(!query.is_sensitive(), "Query Hydrus waits for 6.g");
        assert!(
            (edge_alpha(&query) - pal.ink_46.3).abs() < 0.02,
            "{name}: a disabled hollow control keeps its ink-46 border ({}), got {}",
            pal.ink_46.3,
            edge_alpha(&query)
        );
        folder.set_state_flags(gtk4::StateFlags::PRELIGHT, false);
        pump_for(100);
        // The hover wash (ink-10) is painted under the border, so the edge pixel is the two over each other.
        let under_wash = 1.0 - (1.0 - pal.ink_58.3) * (1.0 - pal.ink_10.3);
        assert!(
            (edge_alpha(&folder) - under_wash).abs() < 0.02,
            "{name}: and ink-58 over the ink-10 wash ({under_wash:.3}) under the pointer, got {}",
            edge_alpha(&folder)
        );
        f.close();
    }
}

/// DAN-2242 fix 6 (design gate 2.6): the production prompter shows the default button's focus ring
/// as soon as the dialog opens, and the default of a destructive question is No.
fn a_dialog_shows_the_ring_on_its_default(app: &adw::Application) {
    use hatate_gtk::dialogs::{AdwPrompter, Prompter};
    let f = fx(app);
    assert!(
        !f.w.win.upcast_ref::<gtk4::Window>().gets_focus_visible(),
        "no key has been pressed yet"
    );
    let prompter = AdwPrompter::new(&f.w.win);
    let answered = Rc::new(Cell::new(None));
    let a = answered.clone();
    prompter.message(
        Kind::Question,
        "Clear Saved Session",
        "Empty the current list?",
        Buttons::YesNo,
        Box::new(move |r| a.set(Some(r))),
    );
    pump_for(400);
    assert!(
        f.w.win.upcast_ref::<gtk4::Window>().gets_focus_visible(),
        "the ring is drawn on the dialog's default button the moment it opens"
    );
    let focused = gtk4::prelude::GtkWindowExt::focus(f.w.win.upcast_ref::<gtk4::Window>())
        .expect("focus is inside the dialog");
    let mut label = String::new();
    let mut stack = vec![focused];
    while let Some(w) = stack.pop() {
        if let Some(l) = w.downcast_ref::<gtk4::Label>() {
            label = l.text().to_string();
            break;
        }
        let mut c = w.first_child();
        while let Some(ch) = c {
            stack.push(ch.clone());
            c = ch.next_sibling();
        }
    }
    assert_eq!(
        label, "No",
        "the default button, and the focused one, is No"
    );
    assert_eq!(answered.get(), None, "nothing answered the question");
    f.close();
}

/// `LensConnection` reaches the window from the Lens client's thread through the inbox and is said
/// in the status line; `Ready` says nothing (Lens answered, nothing to report).
fn lens_connection_is_said_in_the_status_line(app: &adw::Application) {
    use hatate_core::engines::lens_search::LensConnection as C;
    use hatate_gtk::search_run::BusEvent;
    let f = fx(app);
    let bus = f.w.s.search.bus.clone();
    let say_after = |c: C| {
        f.w.say("Ready");
        // From another thread, as the Lens client posts it.
        let b = bus.clone();
        std::thread::spawn(move || b.post(BusEvent::Lens(c)))
            .join()
            .unwrap();
        pump_for(60);
        f.say()
    };
    assert_eq!(
        say_after(C::NotReachable),
        "Google Lens: the browser could not be reached - see Help > View Logs"
    );
    assert_eq!(
        say_after(C::WaitingForLens),
        "Google Lens: waiting for an answer\u{2026}"
    );
    assert_eq!(
        say_after(C::Blocked),
        "Google Lens: standing down for this run"
    );
    assert_eq!(say_after(C::Ready), "Ready");
    assert_eq!(f.w.s.search.lens.get(), Some(C::Ready));
    f.close();
}

/// DAN-2245 item 1: the log viewer opens with focus on Close, nothing selected, and selected text
/// (the path line, the log body; focused or not) is ink on an ink-18 wash, at least 4.5:1, not
/// libadwaita's accent blue (3.48:1 dark, 3.40:1 light under the path line), wherever the toolkit
/// paints a selection in this headless session (GTK 4.24 does; 4.14 paints none here, and its
/// numbers come from the shot set).
fn the_log_viewer_opens_on_close_and_selects_in_ink(app: &adw::Application) {
    use hatate_gtk::tokens::{Palette, Rgba, Scheme, contrast};
    for (name, scheme) in [("dark", Scheme::Dark), ("light", Scheme::Light)] {
        let f = Fx::new(
            app,
            Opts {
                tweak: Some(Box::new(move |c| c.theme = name.to_owned())),
                ..Opts::default()
            },
        );
        f.activate("view-logs");
        pump_for(400);
        let viewer = f.w.s.log_viewer.borrow().clone().expect("the log viewer");
        let pal = Palette::of(scheme);
        let focus = GtkWindowExt::focus(&viewer.window).expect("the viewer has a focus");
        assert_eq!(
            focus
                .downcast_ref::<gtk4::Button>()
                .and_then(|b| b.label())
                .as_deref(),
            Some("Close"),
            "{name}: the viewer opens on Close, not on the selectable path line"
        );
        assert_eq!(
            viewer.path_label.selection_bounds(),
            None,
            "{name}: and nothing is selected"
        );

        // What a selection looks like: the background pixel is the wash over the surface under it,
        // and the pixel farthest from it (a glyph) clears 4.5:1.
        // The widget's own render has no surface under it: lay each pixel over the one it sits on.
        // An unfocused selection is not painted at all by every toolkit (GTK 4.14 draws none for a label
        // that is not focused), so it is only read when something is painted; a focused one must be.
        // Every state is read before the scenario fails, so one run on a new toolkit lists them all.
        let problems = std::cell::RefCell::new(Vec::<String>::new());
        let check = |what: &str,
                     w: &gtk4::Widget,
                     surface: Rgba,
                     crop: (usize, usize),
                     must_paint: bool| {
            let to_rgba = |p: [u8; 4]| {
                Rgba(
                    f64::from(p[0]),
                    f64::from(p[1]),
                    f64::from(p[2]),
                    f64::from(p[3]) / 255.0,
                )
                .over(surface)
            };
            pump_for(150);
            let img = render(w, 0);
            let wash = pal.ink_18.over(surface);
            // The most common pixel of a selected line is its background.
            let mut seen: std::collections::HashMap<[u8; 4], usize> = Default::default();
            let (cw, ch) = (img.0.min(crop.0), img.1.min(crop.1));
            for i in 0..cw * ch {
                *seen.entry(px(&img, i % cw, i / cw)).or_default() += 1;
            }
            let bg = to_rgba(*seen.iter().max_by_key(|(_, n)| **n).expect("pixels").0);
            if bg.hex() == surface.hex() {
                // GTK 4.14 paints no selection in a label or view that this headless session has not
                // made "active" (found by hosted ci on DAN-2245): nothing to measure. The rule itself
                // is pinned in `theme::tests` and the shot set measures what each toolkit draws.
                eprintln!(
                    "note: {name}: {what}: no selection is painted on this toolkit ({must_paint})"
                );
                return;
            }
            if (bg.0 - wash.0).abs() > 1.0
                || (bg.1 - wash.1).abs() > 1.0
                || (bg.2 - wash.2).abs() > 1.0
            {
                problems.borrow_mut().push(format!(
                    "{name}: {what}: the selection is the ink-18 wash {}, got {}",
                    wash.hex(),
                    bg.hex()
                ));
            }
            let worst = (0..cw * ch)
                .map(|i| contrast(to_rgba(px(&img, i % cw, i / cw)), wash))
                .fold(0.0_f64, f64::max);
            if worst < 4.5 {
                problems.borrow_mut().push(format!(
                    "{name}: {what}: selected text reaches {worst:.2}:1, below 4.5"
                ));
            }
        };
        // The log body is one short line in a tall view: read the first line's box only.
        // The path line, selected while Close holds focus (the window is never active under Xvfb,
        // so this is the backdrop state), then focused.
        viewer.path_label.select_region(0, -1);
        check(
            "path line",
            viewer.path_label.upcast_ref(),
            pal.page,
            (usize::MAX, usize::MAX),
            false,
        );
        viewer.path_label.grab_focus();
        viewer.path_label.select_region(0, -1);
        check(
            "path line, focused",
            viewer.path_label.upcast_ref(),
            pal.page,
            (usize::MAX, usize::MAX),
            true,
        );
        // The log body.
        let buffer = viewer.text.buffer();
        buffer.select_range(&buffer.start_iter(), &buffer.end_iter());
        check(
            "log body",
            viewer.text.upcast_ref(),
            pal.sunken,
            (130, 14),
            false,
        );
        viewer.text.grab_focus();
        buffer.select_range(&buffer.start_iter(), &buffer.end_iter());
        check(
            "log body, focused",
            viewer.text.upcast_ref(),
            pal.sunken,
            (130, 14),
            true,
        );
        assert!(
            problems.borrow().is_empty(),
            "{}",
            problems.borrow().join("; ")
        );
        viewer.window.close();
        f.close();
    }
}

fn queue_view_holds_5000_rows_and_filters_without_touching_the_queue(app: &adw::Application) {
    use hatate_core::entry_filter::EntryFilter;
    use hatate_core::table_cells::Column;
    let mut seed = seeded(5000, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % MatchStatus::ALL.len()];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    assert_eq!(view.total_count(), 5000);
    assert_eq!(view.visible_count(), 5000);
    assert_eq!(view.cell(0, Column::File).unwrap().text, "img0.png");
    assert_eq!(view.cell(1, Column::Status).unwrap().text, "\u{25cf} Good");
    // A filter hides rows from the table, never from the queue.
    view.set_filter(EntryFilter {
        statuses: Some([MatchStatus::Good].into_iter().collect()),
        ..EntryFilter::default()
    });
    assert_eq!(view.visible_count(), 5000 / 6 + usize::from(5000 % 6 > 1));
    assert_eq!(f.w.s.queue.borrow().len(), 5000);
    assert_eq!(view.cell(0, Column::File).unwrap().text, "img1.png");
    // The selection resolves through the screen, so a hidden row cannot be reached.
    view.select_row(0);
    let id = view.selected_ids()[0];
    assert_eq!(
        f.w.s.queue.borrow().entry(id).unwrap().path,
        "/lib/img1.png"
    );
    view.set_filter(EntryFilter::default());
    assert_eq!(
        view.selected_ids(),
        vec![id],
        "the selection survives a reset"
    );
    assert_eq!(view.row_of(id), Some(1));
    f.close();
}

fn queue_filter_bar_narrows_by_text_and_menus_reads_hidden_and_clears(app: &adw::Application) {
    let mut seed = seeded(12, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % 3];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let bar = f.w.s.filter_bar.clone();
    assert_eq!(view.visible_count(), 12);
    assert_eq!(bar.count.label().as_str(), "", "no filter, no reading");
    assert!(!bar.clear.is_sensitive());
    assert_eq!(
        bar.button("Status").label().unwrap().as_str(),
        "Status: All"
    );

    // Typing narrows the list by filename and the reading says how many are held back.
    bar.text.set_text("img1");
    assert_eq!(view.visible_count(), 3, "img1, img10 and img11");
    assert_eq!(bar.count.label().as_str(), "9 hidden");
    assert!(bar.clear.is_sensitive());
    assert_eq!(
        bar.count.tooltip_text().unwrap().as_str(),
        "showing 3 of 12 (the rest are hidden, not removed)"
    );
    bar.clear.emit_clicked();
    assert_eq!(view.visible_count(), 12);
    assert_eq!(bar.text.text().as_str(), "");
    assert_eq!(bar.count.label().as_str(), "");

    // The Status menu: None empties the list, ticking one back isolates it, All restores it.
    let status = bar.button("Status");
    status.activate_action("fstatus.none", None).unwrap();
    assert_eq!(view.visible_count(), 0);
    assert_eq!(status.label().unwrap().as_str(), "Status: 0 selected");
    status.activate_action("fstatus.v0", None).unwrap();
    assert_eq!(view.visible_count(), 4);
    assert_eq!(status.label().unwrap().as_str(), "Status: Not searched");
    status.activate_action("fstatus.all", None).unwrap();
    assert_eq!(view.visible_count(), 12);
    assert_eq!(status.label().unwrap().as_str(), "Status: All");
    // A filter hides rows from the table, never from the queue.
    assert_eq!(f.w.s.queue.borrow().len(), 12);
    f.close();
}

fn queue_select_by_status_under_a_filter_never_selects_a_hidden_row(app: &adw::Application) {
    // DAN-2310 class 1: Python selects the rows its filter hides, and Remove / Reset / Mark
    // reviewed then act on rows nobody can see. Here a hidden row is never selected.
    let mut seed = seeded(12, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % 3];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let bar = f.w.s.filter_bar.clone();
    // What a save would write: the rows, their statuses and their reviewed marks.
    let snapshot = |f: &Fx| -> Vec<(String, MatchStatus, bool)> {
        f.w.s
            .queue
            .borrow()
            .entries()
            .iter()
            .map(|e| (e.path.clone(), e.status, e.reviewed))
            .collect()
    };
    let before = snapshot(&f);

    // "img1" shows img1, img10 and img11 (statuses 1, 1, 2); the four rows of status 0 are hidden.
    bar.text.set_text("img1");
    assert_eq!(view.visible_count(), 3);
    let hidden_status = MatchStatus::ALL[0];
    f.w.select_rows_by_status(hidden_status);
    assert!(view.selected_ids().is_empty(), "a hidden row was selected");
    assert_eq!(
        f.say(),
        format!(
            "No visible images matching status \"{}\" (4 hidden by the filter)",
            hidden_status.label()
        )
    );
    // Remove and Mark reviewed act on the (empty) selection and change nothing.
    let none = view.selected_ids();
    f.w.remove_rows(&none);
    f.w.set_rows_reviewed(&none, true);
    assert_eq!(
        snapshot(&f),
        before,
        "the queue changed under an empty selection"
    );

    // A status that is partly visible selects only the visible rows and counts only those.
    let part = MatchStatus::ALL[1];
    f.w.select_rows_by_status(part);
    assert_eq!(
        view.selected_ids().len(),
        2,
        "img1 and img10, not img4 or img7"
    );
    assert_eq!(
        f.say(),
        format!("Selected 2 image(s) matching status \"{}\"", part.label())
    );
    // Reset on every visible row leaves the hidden ones as they were
    // (`test_resetting_only_resets_visible_rows`).
    f.w.select_rows_by_predicate("every visible row", |_| true);
    let shown_ids = view.selected_ids();
    assert_eq!(shown_ids.len(), 3, "img1, img10 and img11");
    let before_reset = snapshot(&f);
    f.script.push(Answer::Msg(Response::Yes));
    f.w.reset_rows(&shown_ids);
    for ((path, status, _), (_, old_status, _)) in snapshot(&f).iter().zip(&before_reset) {
        if path.contains("img1") {
            assert_eq!(*status, MatchStatus::NotSearched, "{path} is visible");
        } else {
            assert_eq!(
                status, old_status,
                "reset a row the filter had hidden: {path}"
            );
        }
    }
    // With no filter nothing is hidden, so the old wording stands.
    bar.clear.emit_clicked();
    f.w.select_rows_by_predicate("nothing", |_| false);
    assert_eq!(f.say(), "No images matching nothing");
    f.close();
}

fn queue_clear_re_ticks_every_filter_menu_so_the_ticks_summary_and_count_agree(
    app: &adw::Application,
) {
    // DAN-2310 class 2: Python's Clear leaves the Status / Site / Upscale ticks as they were,
    // so toggling a stale tick does the opposite of what it shows. Here the ticks follow the filter.
    let mut seed = seeded(12, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % 3];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let bar = f.w.s.filter_bar.clone();
    let status = bar.button("Status");
    status.activate_action("fstatus.v0", None).unwrap();
    assert!(!bar.ticked("Status", 0), "the tick was not removed");
    assert_eq!(view.visible_count(), 8);
    assert_eq!(bar.count.label().as_str(), "4 hidden");
    bar.text.set_text("img1");
    assert!(bar.clear.is_sensitive());

    bar.clear.emit_clicked();
    for prefix in ["Status", "Site", "Upscale"] {
        for n in 0..bar.present(prefix).len() {
            assert!(
                bar.ticked(prefix, n),
                "{prefix} entry {n} is still unticked after Clear"
            );
        }
    }
    assert_eq!(status.label().unwrap().as_str(), "Status: All");
    assert_eq!(bar.text.text().as_str(), "");
    assert_eq!(view.visible_count(), 12);
    assert_eq!(bar.count.label().as_str(), "");
    assert!(!bar.clear.is_sensitive());
    f.close();
}

fn queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row(
    app: &adw::Application,
) {
    use hatate_core::table_cells::Column;
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(5, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let ids = view.visible_ids();
    let path_of = |id| f.w.s.queue.borrow().entry(id).unwrap().path.clone();

    // `_successor_surviving`: removing the selected row selects the one that followed it.
    view.select_row(1);
    f.script.push(Answer::Msg(Response::Remove));
    f.w.remove_rows(&[ids[1]]);
    assert_eq!(
        view.selected_ids()
            .iter()
            .map(|i| path_of(*i))
            .collect::<Vec<_>>(),
        ["/lib/img2.png"]
    );
    // The last row has nothing below it, so the nearest one above is taken.
    view.select_row(3);
    f.script.push(Answer::Msg(Response::Remove));
    f.w.remove_rows(&[ids[4]]);
    assert_eq!(
        view.selected_ids()
            .iter()
            .map(|i| path_of(*i))
            .collect::<Vec<_>>(),
        ["/lib/img3.png"]
    );
    // A selection that survives is left as it is.
    view.select_row(0);
    f.script.push(Answer::Msg(Response::Remove));
    f.w.remove_rows(&[ids[2]]);
    assert_eq!(
        view.selected_ids()
            .iter()
            .map(|i| path_of(*i))
            .collect::<Vec<_>>(),
        ["/lib/img0.png"]
    );

    // `_apply_sort`: the top selected row is the current one and the only one left selected.
    let top = view.visible_ids();
    view.select_ids(&[top[0], top[1]]);
    view.on_header_clicked(Column::File);
    assert_eq!(view.selected_ids().len(), 1);
    assert_eq!(path_of(view.selected_ids()[0]), "/lib/img0.png");
    f.close();
}

fn queue_view_thumbnails_load_fail_and_stay_off_the_main_loop(app: &adw::Application) {
    use hatate_core::thumb::{FAILED_TOOLTIP, ThumbState};
    let pics = tempfile::tempdir().unwrap();
    let good = pics.path().join("good.png");
    image::RgbaImage::from_pixel(640, 320, image::Rgba([10, 120, 200, 255]))
        .save(&good)
        .unwrap();
    let text = pics.path().join("text.png");
    std::fs::write(&text, b"not a picture").unwrap();
    let mut seed = seeded(3, MatchStatus::Good);
    seed[0].path = good.to_string_lossy().into_owned();
    seed[1].path = text.to_string_lossy().into_owned();
    seed[2].path = pics
        .path()
        .join("missing.png")
        .to_string_lossy()
        .into_owned();
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let ids = view.visible_ids();
    // Every drawn row asks, so all three settle: one picture, two failures.
    assert!(
        pump_until(10_000, || ids.iter().all(|id| view.thumb_known(*id))),
        "the drawn rows were never decoded"
    );
    assert_eq!(
        ids.iter()
            .map(|id| view.thumb_state(*id))
            .collect::<Vec<_>>(),
        [ThumbState::Loaded, ThumbState::Failed, ThumbState::Failed]
    );
    assert_eq!(f.w.s.thumbs.in_flight(), 0);
    assert!(FAILED_TOOLTIP.len() > 10);
    // A row rebuilt from the cache keeps its picture without a second decode.
    view.rebuild();
    assert_eq!(view.thumb_state(ids[0]), ThumbState::Loaded);
    assert!(!f.w.s.thumbs.is_running());
    f.close();
}

fn queue_view_thumbnails_stay_off_when_the_source_is_off(app: &adw::Application) {
    // `test_off_reads_no_files_at_all_but_still_finishes`: no file is read, no pass is left
    // running, and the progress strip is not left showing a pass that never ends.
    let pics = tempfile::tempdir().unwrap();
    let good = pics.path().join("good.png");
    image::RgbaImage::from_pixel(64, 64, image::Rgba([10, 120, 200, 255]))
        .save(&good)
        .unwrap();
    let mut seed = seeded(2, MatchStatus::Good);
    seed[0].path = good.to_string_lossy().into_owned();
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            tweak: Some(Box::new(|s| s.thumbnail_source = "off".into())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    pump_for(500);
    for id in view.visible_ids() {
        assert!(
            !view.thumb_known(id),
            "a picture was decoded with the source off"
        );
    }
    assert_eq!(f.w.s.thumbs.in_flight(), 0);
    assert!(!f.w.s.thumbs.is_running());
    f.close();
}

fn queue_context_menu_selects_marks_resets_and_removes(app: &adw::Application) {
    use hatate_core::table_menu::Predicate;
    let mut seed = seeded(12, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % MatchStatus::ALL.len()];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    // Nothing selected: only the four "Select by" menus.
    assert!(f.w.table_menu(&[]).rows.is_none());

    f.w.select_rows_by_status(MatchStatus::Good);
    assert_eq!(view.selected_ids().len(), 2);
    assert_eq!(
        f.say(),
        "Selected 2 image(s) matching status \"Found (good)\""
    );
    f.w.select_rows_by_predicate(Predicate::MissingFromDisk.description(), |e| {
        Predicate::MissingFromDisk.test(e)
    });
    assert!(
        view.selected_ids().is_empty(),
        "a new selection replaces the old one"
    );
    assert_eq!(f.say(), "No images matching Missing from disk");

    // The menu over those two rows says "2", and its wired entries have handlers.
    f.w.select_rows_by_status(MatchStatus::Good);
    let ids = view.selected_ids();
    let rows = f.w.table_menu(&ids).rows.unwrap();
    assert!(rows.tail.iter().any(|l| matches!(l,
        hatate_core::table_menu::Line::Item(i) if i.label == "Remove 2 selected files")));

    // Reviewed: no question, the status line counts what is left to review.
    f.w.set_rows_reviewed(&ids, true);
    assert!(
        f.say()
            .starts_with("Marked 2 image(s) as reviewed \u{2014} ")
    );
    f.w.set_rows_reviewed(&ids, true);
    assert!(
        f.say().starts_with("Marked 2"),
        "nothing changed, nothing said"
    );

    // Reset asks first, and the question defaults to No.
    f.w.reset_rows(&ids);
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.buttons, shown.default),
        (Buttons::YesNo, Response::No)
    );
    assert!(
        shown
            .text
            .starts_with("Discard the search results for 2 images?")
    );
    assert!(
        view.cell(1, hatate_core::table_cells::Column::Status)
            .is_some()
    );
    f.script.push(Answer::Msg(Response::Yes));
    f.w.reset_rows(&ids);
    assert_eq!(f.say(), "Reset 2 images to Not searched");

    // Remove asks with Cancel as the default, and acts only on Remove.
    f.w.remove_rows(&ids);
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.buttons, shown.default),
        (Buttons::RemoveCancel, Response::Cancel)
    );
    assert_eq!(f.len(), 12, "the default answer removes nothing");
    f.script.push(Answer::Msg(Response::Remove));
    f.w.remove_rows(&ids);
    assert_eq!(f.len(), 10);
    assert_eq!(f.say(), "Removed 2 file(s) from the list");
    f.close();
}

fn queue_view_header_cycle_sorts_the_queue_in_place_and_restores_the_order(app: &adw::Application) {
    use hatate_core::table_cells::Column;
    let mut seed = seeded(4, MatchStatus::Good);
    for (e, name) in seed
        .iter_mut()
        .zip(["/lib/c.png", "/lib/A.png", "/lib/b.png", "/lib/d.png"])
    {
        e.path = name.to_owned();
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let names = || -> Vec<String> {
        f.w.s
            .queue
            .borrow()
            .entries()
            .iter()
            .map(|e| e.filename().to_owned())
            .collect()
    };
    assert_eq!(names(), ["c.png", "A.png", "b.png", "d.png"]);
    view.on_header_clicked(Column::File);
    assert_eq!(
        names(),
        ["A.png", "b.png", "c.png", "d.png"],
        "ascending, case-folded"
    );
    assert_eq!(view.cell(0, Column::File).unwrap().text, "A.png");
    view.on_header_clicked(Column::File);
    assert_eq!(names(), ["d.png", "c.png", "b.png", "A.png"], "descending");
    view.on_header_clicked(Column::File);
    assert_eq!(view.sort_state(), None);
    assert_eq!(
        names(),
        ["c.png", "A.png", "b.png", "d.png"],
        "back to insertion order"
    );
    f.close();
}

fn tagged_seed(dir: &std::path::Path, names: &[&str]) -> Vec<hatate_core::model::Entry> {
    make_images(dir, names)
        .into_iter()
        .map(|p| {
            let mut e = entry(&p.to_string_lossy(), MatchStatus::Good);
            e.tags = vec![hatate_core::model::Tag {
                name: "cat".to_owned(),
                namespace: None,
                source: hatate_core::model::TagSource::User,
            }];
            e
        })
        .collect()
}

fn export_results_writes_the_format_the_name_says(app: &adw::Application) {
    // An empty list says so and asks nothing.
    let f = fx(app);
    f.activate("export-results");
    assert_eq!(
        f.script.last_shown().text,
        "There's nothing in the list to export."
    );
    assert!(f.script.asked.borrow().is_empty());
    f.close();

    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.script.push(Answer::Path(Some(f.dir.path().join("out"))));
    f.activate("export-results");
    let csv = f.dir.path().join("out.csv");
    let text = std::fs::read_to_string(&csv).expect("the .csv is added to a bare name");
    assert!(
        text.contains("\r\n") && text.contains("img0.png"),
        "{text:?}"
    );
    assert_eq!(f.say(), "Exported 2 image(s) to out.csv");
    let Asked::SaveFile {
        title,
        default_name,
        filter,
    } = f.script.asked.borrow().last().cloned().unwrap()
    else {
        panic!("not a save chooser")
    };
    assert_eq!(title, "Export Results");
    assert!(
        default_name.contains("hatate-results-") && default_name.ends_with(".csv"),
        "{default_name}"
    );
    assert_eq!(
        filter,
        "CSV spreadsheet (*.csv);;JSON (*.json);;All files (*)"
    );
    // A name that says JSON, with no filter to say otherwise, writes JSON.
    f.script
        .push(Answer::Path(Some(f.dir.path().join("out.json"))));
    f.activate("export-results");
    let json = std::fs::read_to_string(f.dir.path().join("out.json")).unwrap();
    assert!(
        json.starts_with('{') && json.contains("\"count\": 2"),
        "{json:?}"
    );
    // An unwritable path warns and says nothing was exported.
    f.script.push(Answer::Path(Some(PathBuf::from(
        "/nonexistent-dir-for-test/x.csv",
    ))));
    f.activate("export-results");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.kind, shown.title.as_str()),
        (Kind::Warning, "Export Results")
    );
    assert!(
        shown
            .text
            .starts_with("Could not write the results to:\n/nonexistent-dir-for-test/x.csv\n\n")
    );
    assert!(!shown.text.contains("os error"), "{}", shown.text);
    f.close();
}

fn write_tag_files_asks_then_writes_and_keeps_what_is_there(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let seed = tagged_seed(dir.path(), &["a.png", "b.png"]);
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    std::fs::write(dir.path().join("a.png.txt"), "mine\n").unwrap();
    // `ask` with a file in the way: the three-way box, Keep existing files the default.
    f.script.push(Answer::Msg(Response::Keep));
    f.activate("write-tag-files");
    let shown = f.script.last_shown();
    assert_eq!(shown.buttons, Buttons::KeepReplaceCancel);
    assert_eq!(shown.default, Response::Keep);
    assert!(
        shown.text.contains("1 of them already exist, for example:"),
        "{}",
        shown.text
    );
    assert!(shown.text.ends_with("This app cannot tell a tag file it wrote on an earlier run from a text file you wrote yourself."));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.png.txt")).unwrap(),
        "mine\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.png.txt")).unwrap(),
        "cat\n"
    );
    assert_eq!(
        f.say(),
        "Wrote 1 tag file(s), 1 already existed and were kept"
    );
    // Replace them replaces; Cancel writes nothing.
    f.script.push(Answer::Msg(Response::Cancel));
    std::fs::remove_file(dir.path().join("b.png.txt")).unwrap();
    f.activate("write-tag-files");
    assert!(!dir.path().join("b.png.txt").exists());
    f.script.push(Answer::Msg(Response::Replace));
    f.activate("write-tag-files");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.png.txt")).unwrap(),
        "cat\n"
    );
    assert_eq!(f.say(), "Wrote 2 tag file(s)");
    f.close();
}

fn clear_search_cache_asks_then_removes_the_saved_results(app: &adw::Application) {
    let f = fx(app);
    // Set at startup, before anything is cached.
    assert_eq!(
        f.w.s.clear_cache_item.label(),
        "Clear Search Cache (0 B)\u{2026}"
    );
    let cache = hatate_core::cache::SearchCache::new(&f.paths);
    std::fs::create_dir_all(cache.dir()).unwrap();
    for n in ["a", "b"] {
        std::fs::write(cache.dir().join(format!("{n}.json")), "{}").unwrap();
    }
    // The question defaults to No (design gate 2.6), so nothing is deleted without a Yes.
    f.activate("clear-search-cache");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.buttons, shown.default),
        (Buttons::YesNo, Response::No)
    );
    assert_eq!(std::fs::read_dir(cache.dir()).unwrap().count(), 2);
    f.script.push(Answer::Msg(Response::Yes));
    f.activate("clear-search-cache");
    assert_eq!(
        f.script.last_shown().text,
        "Removed 2 cached search result(s)."
    );
    assert_eq!(std::fs::read_dir(cache.dir()).unwrap().count(), 0);
    f.close();
}

fn queue_header_layout_is_saved_on_close_and_restored_repaired_or_discarded(
    app: &adw::Application,
) {
    use hatate_core::table_cells::{
        COLUMNS, DEFAULT_COLUMN_WIDTH, HEADER_STATE_PREFIX, HeaderSection, encode_header_state,
    };
    let saved = |paths: &hatate_core::paths::Paths| {
        let s = hatate_core::config::Settings::load(paths).settings;
        (s.table_header_state, s.table_header_columns)
    };
    let with_state = |state: String, columns: i64| Opts {
        tweak: Some(Box::new(move |s| {
            s.table_header_state = state.clone();
            s.table_header_columns = columns;
        })),
        ..Opts::default()
    };

    // A usable layout, with one hidden and one too-narrow column: restored, then repaired.
    let mut sections = vec![
        HeaderSection {
            width: 130,
            hidden: false
        };
        COLUMNS.len()
    ];
    sections[2].hidden = true;
    sections[3].width = 10;
    let f = Fx::new(app, with_state(encode_header_state(&sections), 12));
    let got = f.w.s.queue_view.header_sections();
    assert!(!got[2].hidden, "the hidden column is shown again");
    assert_eq!(
        got[3].width, DEFAULT_COLUMN_WIDTH,
        "the narrow column is widened"
    );
    assert_eq!(
        got[4].width, 130,
        "an ordinary column keeps its saved width"
    );
    // A saved layout is not overwritten by the window changing size
    // (`test_a_saved_layout_is_not_overwritten_by_the_defaults`).
    f.w.win.set_default_size(1500, 900);
    pump_for(300);
    let resized = f.w.s.queue_view.header_sections();
    assert_eq!(
        resized[4].width, 130,
        "a resize rewrote a saved column width"
    );
    f.w.close_teardown();
    // Closing saves the live layout in this build's format, with the column count. Read before the
    // fixture closes: dropping it removes the profile.
    let (state, columns) = saved(&f.paths);
    f.close();
    assert!(state.starts_with(HEADER_STATE_PREFIX), "{state:?}");
    assert_eq!(columns, 12);
    assert_eq!(state.split(',').count(), 12);

    // A Qt blob of the right column count (what a Python build wrote): discarded, never parsed.
    let f = Fx::new(
        app,
        with_state(
            "AAAA/wAAAAAAAAABAAAAAQEAAAAAAAAAAAAAAAAAAAAAAADIAAA=".into(),
            12,
        ),
    );
    assert_eq!(f.w.s.settings.lock().unwrap().table_header_state, "");
    assert_eq!(f.w.s.settings.lock().unwrap().table_header_columns, 12);
    f.close();

    // Saved with another column count: both keys reset.
    let f = Fx::new(app, with_state(encode_header_state(&sections), 10));
    let s = f.w.s.settings.lock().unwrap().clone();
    assert_eq!(
        (s.table_header_state.as_str(), s.table_header_columns),
        ("", 0)
    );
    drop(s);
    f.close();
}

fn clear_search_cache_label_carries_the_cache_size(app: &adw::Application) {
    let f = fx(app);
    let cache = hatate_core::cache::SearchCache::new(&f.paths);
    std::fs::create_dir_all(cache.dir()).unwrap();
    std::fs::write(
        cache.dir().join(format!("{}.json", "a".repeat(64))),
        vec![b'x'; 1536],
    )
    .unwrap();
    // The label is refreshed when the menu is about to open; here the refresh is called directly.
    f.w.update_clear_cache_label();
    assert_eq!(
        f.w.s.clear_cache_item.label(),
        "Clear Search Cache (1.5 KB)\u{2026}"
    );
    f.close();
}

/// Every `gtk::Label` under `w` (itself included).
fn all_labels(w: &gtk4::Widget, out: &mut Vec<gtk4::Label>) {
    if let Some(l) = w.downcast_ref::<gtk4::Label>() {
        out.push(l.clone());
    }
    let mut child = w.first_child();
    while let Some(c) = child {
        all_labels(&c, out);
        child = c.next_sibling();
    }
}

/// DAN-1999 F6: the `THUMB` title (letter-spaced, in a 72 px column) was clipped at the minimum
/// window size. Every column title must be drawn whole: not ellipsized, and given at least the
/// width it asks for.
fn queue_header_titles_are_not_clipped_at_the_minimum_window_size(app: &adw::Application) {
    use hatate_core::table_cells::Column;
    for scheme in ["dark", "light"] {
        let s = scheme.to_owned();
        let f = Fx::new(
            app,
            Opts {
                seed: seeded(5, MatchStatus::Good),
                missing: Some(Arc::new(|_, _| HashSet::new())),
                tweak: Some(Box::new(move |c| c.theme = s.clone())),
                ..Opts::default()
            },
        );
        f.w.win.set_default_size(1, 1);
        pump_for(300);
        let mut labels = Vec::new();
        all_labels(f.w.s.queue_view.view.upcast_ref(), &mut labels);
        let mut seen = 0;
        for col in Column::ALL {
            let title = col.title();
            let Some(label) = labels
                .iter()
                .find(|l| l.text() == title && l.is_mapped() && l.width() > 0)
            else {
                panic!("{scheme}: no drawn header title {title:?}");
            };
            seen += 1;
            // The header cell is the enclosing button: the title must sit wholly inside it.
            let mut cell = label.parent();
            while let Some(c) = &cell {
                if c.css_name() == "button" {
                    break;
                }
                cell = c.parent();
            }
            let cell = cell.unwrap_or_else(|| panic!("{scheme}: header {title:?} has no cell"));
            let b = label.compute_bounds(&cell).expect("bounds");
            println!(
                "HEADER {scheme} {title} cell_width={} label_x={:.1}..{:.1} ellipsized={}",
                cell.width(),
                b.x(),
                b.x() + b.width(),
                label.layout().is_ellipsized()
            );
            assert!(
                !label.layout().is_ellipsized()
                    && b.x() >= -0.5
                    && b.x() + b.width() <= cell.width() as f32 + 0.5,
                "{scheme}: header {title:?} is clipped (cell {}, title {:.1}..{:.1})",
                cell.width(),
                b.x(),
                b.x() + b.width()
            );
        }
        assert_eq!(seen, Column::ALL.len());
        f.close();
    }
}

/// The first label under `w` that is a `GtkModelButton` text, per visible popover menu: what menus
/// are open, by their first entry.
fn open_menu_first_entries(w: &gtk4::Widget, out: &mut Vec<String>) {
    if let Some(p) = w.downcast_ref::<gtk4::PopoverMenu>() {
        if p.is_visible() {
            out.push(first_model_button_text(p.upcast_ref()).unwrap_or_default());
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        open_menu_first_entries(&child, out);
        c = child.next_sibling();
    }
}

fn first_model_button_text(w: &gtk4::Widget) -> Option<String> {
    if w.type_().name() == "GtkModelButton" {
        return w.property::<Option<String>>("text");
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(t) = first_model_button_text(&child) {
            return Some(t);
        }
        c = child.next_sibling();
    }
    None
}

/// DAN-2246: the menubar by keyboard, with real X key events (`common::xtest`) delivered to the
/// real window. GTK does Alt+mnemonic, Left and Right on its own and this pins that it still does;
/// `menubar_keys` adds F10 (open the first menu, again to close) and the return of focus to the
/// widget that had it before the bar was entered.
fn menubar_keyboard_path(app: &adw::Application) {
    use common::xtest::{Key, Mod, Xtest};
    let mut keys = match Xtest::connect() {
        Ok(x) => x,
        Err(why) => {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI is set and the X server cannot inject keys: {why}: failing closed"
            );
            println!("menubar_keyboard_path: skipped ({why})");
            return;
        }
    };
    let f = fx(app);
    // Earlier scenarios leave their secondary windows mapped (the log viewer); they would hold the
    // keyboard. Only this scenario's window may be on screen.
    for w in gtk4::Window::list_toplevels() {
        if &w != f.w.win.upcast_ref::<gtk4::Widget>() {
            if let Ok(w) = w.downcast::<gtk4::Window>() {
                w.destroy();
            }
        }
    }
    f.w.win.set_default_size(1100, 800);
    pump_for(400);
    let root = f.w.win.clone().upcast::<gtk4::Widget>();
    let window = f.w.win.upcast_ref::<gtk4::Window>();
    let focus = || gtk4::prelude::GtkWindowExt::focus(window);
    let open_menus = || {
        let mut v = Vec::new();
        open_menu_first_entries(&root, &mut v);
        v
    };
    let files = "Add Files\u{2026}";
    let hydrus = "Send File + URL + Tags to Hydrus (upload)";

    // No window manager under xvfb: the pointer and the X focus put the keys on this window.
    keys.focus_window_at(300, 300).expect("focus the window");
    let add = find_button(&root, "Add files\u{2026}").expect("Add files");
    add.grab_focus();
    pump_for(200);
    let add_widget = add.clone().upcast::<gtk4::Widget>();
    assert_eq!(
        focus().as_ref(),
        Some(&add_widget),
        "Add files starts with focus"
    );

    let mut press = |key: Key, mods: &[Mod]| {
        keys.tap(key, mods).expect("inject");
        pump_for(400);
    };

    // The keys reach the window at all: Alt+F is GTK's own, and opens Files.
    press(Key::Char('f'), &[Mod::Alt]);
    assert_eq!(open_menus(), [files], "Alt+F opens Files");
    press(Key::Escape, &[]);
    assert!(open_menus().is_empty(), "Escape closes it");
    assert_eq!(
        focus().as_ref(),
        Some(&add_widget),
        "and focus returns to Add files"
    );

    // Shift+F10 is the context-menu key and Ctrl+F10 is not ours: neither opens a menu.
    press(Key::F10, &[Mod::Shift]);
    assert!(open_menus().is_empty(), "Shift+F10 does not open a menu");
    press(Key::F10, &[Mod::Ctrl]);
    assert!(open_menus().is_empty(), "Ctrl+F10 does not open a menu");

    // F10 opens the first menu.
    press(Key::F10, &[]);
    assert_eq!(open_menus(), [files], "F10 opens Files");
    // Right moves to the next menu, Left back.
    press(Key::Right, &[]);
    assert_eq!(
        open_menus(),
        [hydrus],
        "Right opens Hydrus and closes Files"
    );
    press(Key::Left, &[]);
    assert_eq!(open_menus(), [files], "Left goes back to Files");
    // F10 again closes, and focus goes back to where it was before F10.
    press(Key::F10, &[]);
    assert!(open_menus().is_empty(), "F10 a second time closes the menu");
    assert_eq!(
        focus().as_ref(),
        Some(&add_widget),
        "focus returns to Add files after F10, F10"
    );

    // Escape does the same after a trip through two menus.
    press(Key::F10, &[]);
    press(Key::Right, &[]);
    assert_eq!(open_menus(), [hydrus]);
    press(Key::Escape, &[]);
    assert!(open_menus().is_empty(), "Escape closes the Hydrus menu");
    assert_eq!(
        focus().as_ref(),
        Some(&add_widget),
        "Escape returns focus to the widget that had it before F10"
    );
    f.close();
}
