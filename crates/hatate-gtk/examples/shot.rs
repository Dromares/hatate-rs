//! Renders the real window to a PNG, headless (`xvfb-run`), with a temp profile and the bundled
//! fonts: `cargo run --example shot -- OUT.png [dark|light] [WxH|min] [STATE]`. A test-only tool:
//! the shipped binary has no way to do this. Shows only the synthetic profile it creates.
//!
//! `STATE` is one of `empty` (default), `rows`, `crashed`, `survived`, `quota`, `dialog` (the
//! Clear Session question), `logs` (the log viewer), `menu` (the first menu open) or `focus` (the
//! first button focused with the ring showing). The size `min` is the window's stated minimum.

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use hatate_core::config::Settings;
use hatate_core::model::{Entry, MatchStatus};
use hatate_core::paths::Paths;
use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile};
use hatate_core::session::SessionStore;
use hatate_gtk::window::{Env, MIN_HEIGHT, MIN_WIDTH, MainWindow};
use hatate_gtk::{app, fonts};
use std::cell::Cell;
use std::ffi::OsString;
use std::rc::Rc;

const NOW: f64 = 1_792_000_000.0;

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
        "rows" | "dialog" => save(&rows(12)),
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
    env.clock = Rc::new(|| NOW);
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
        let win = window.win.clone();
        let frames = Rc::new(Cell::new(0u32));
        let out = out2.clone();
        win.add_tick_callback(move |win, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 14 {
                return glib::ControlFlow::Continue;
            }
            let paintable = gtk4::WidgetPaintable::new(Some(win));
            let snap = gtk4::Snapshot::new();
            paintable.snapshot(&snap, f64::from(win.width()), f64::from(win.height()));
            let node = snap.to_node().expect("snapshot");
            let renderer = win.native().and_then(|n| n.renderer()).expect("renderer");
            let tex: gdk::Texture = renderer.render_texture(&node, None);
            tex.save_to_png(&out).expect("png");
            println!("SHOT {} {}x{}", out.display(), win.width(), win.height());
            // Where every label is, for the measuring script (`<shot>.rects`, tab separated:
            // x y w h state text); the contrast table is computed from the pixels under each box.
            let mut rects = String::new();
            label_rects(win.upcast_ref(), win.upcast_ref(), &mut rects);
            std::fs::write(out.with_extension("rects"), rects).expect("rects");
            // A popover is its own surface, so the window's snapshot does not hold it.
            if let Some(p) = first_popover_menu(win.upcast_ref()).filter(|p| p.is_visible()) {
                let paintable = gtk4::WidgetPaintable::new(Some(&p));
                let snap = gtk4::Snapshot::new();
                paintable.snapshot(&snap, f64::from(p.width()), f64::from(p.height()));
                if let Some(node) = snap.to_node() {
                    let tex = renderer.render_texture(&node, None);
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
            if let Some(app) = win.application() {
                app.quit();
            }
            glib::ControlFlow::Break
        });
    });
    drop(home);
    code
}

/// What the state does once the window is up.
fn prepare(window: &MainWindow, state: &str) {
    match state {
        "dialog" => window.activate_action("clear-session"),
        "logs" => window.activate_action("view-logs"),
        "menu" => {
            if let Some(p) = first_popover_menu(window.win.upcast_ref()) {
                p.popup();
            }
        }
        "tab" => {
            // Walks Tab from the top and prints what takes focus, for the verdict's Tab order.
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
        "focus" => {
            window.win.set_focus_visible(true);
            if let Some(b) = first_button(window.win.upcast_ref()) {
                b.grab_focus();
            }
        }
        _ => {}
    }
}

/// Every mapped label under `w` with its box in `root`'s coordinates.
fn label_rects(w: &gtk4::Widget, root: &gtk4::Widget, out: &mut String) {
    // A popover is its own surface: its labels are measured against its own snapshot.
    if w.is::<gtk4::PopoverMenu>() && !std::ptr::eq(w.as_ptr(), root.as_ptr()) {
        return;
    }
    if let Some(l) = w.downcast_ref::<gtk4::Label>() {
        let text = l.text();
        if l.is_mapped() && !text.is_empty() {
            if let Some(b) = w.compute_bounds(root) {
                out.push_str(&format!(
                    "{:.0}\t{:.0}\t{:.0}\t{:.0}\t{}\t{}\n",
                    b.x(),
                    b.y(),
                    b.width(),
                    b.height(),
                    if w.is_sensitive() { "on" } else { "disabled" },
                    text.replace(['\t', '\n'], " ")
                ));
            }
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        label_rects(&child, root, out);
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
