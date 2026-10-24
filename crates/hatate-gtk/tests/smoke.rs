//! The window, headless: every scenario builds the real `MainWindow` over a temp profile with a
//! scripted prompter, drives it through its `gio::Action`s and model calls, and reads what it did
//! back. One program, because GTK must run on the main thread and the default test harness does
//! not give a test that.
//!
//! Needs a display. It runs under `xvfb-run` with `HATATE_GTK_SMOKE=1`. **Fails closed**: with
//! `CI` set, a missing flag or a missing display is a failure, not a skip (DAN-1974 item 1).

mod common;

use common::*;
use gtk4::gio;
use gtk4::prelude::*;
use hatate_core::model::MatchStatus;
use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile};
use hatate_core::session::{LoadOptions, SessionStore, named};
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
        for (name, scenario) in SCENARIOS {
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
    assert!(
        f.w.win
            .lookup_action("toggle-search")
            .is_some_and(|a| !a.is_enabled())
    );
    f.close();
}

fn empty_state_and_idle_strip(app: &adw::Application) {
    let f = fx(app);
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
    let quota = hatate_gtk::text::describe_quota_pause(&fresh, NOW);
    // The restore status and the quota text join with a middle dot.
    assert_eq!(
        f.say(),
        format!("Restored 2 image(s) from your last session \u{b7} {quota}")
    );
    assert_eq!(
        f.w.s.shell.run_note_label.text(),
        hatate_gtk::shell::QUOTA_PAUSED_NOTE
    );
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
        let states: [(&str, Build, &str); 6] = [
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
            println!(
                "OUTER {name} {scheme} window={}x{} content={cw}x{ch} needs={w}x{need_h_at_cw}",
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
            worst = (worst.0.max(w), worst.1.max(need_h_at_cw));
            f.close();
        }
    }
    println!(
        "MINSIZE worst content_min={}x{} stated={MIN_WIDTH}x{MIN_HEIGHT}",
        worst.0, worst.1
    );
}
