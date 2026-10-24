//! Renders the real window to a PNG, headless (`xvfb-run`), with a temp profile and the bundled
//! fonts: `cargo run --example shot -- OUT.png [dark|light] [WxH]`. A test-only tool: the shipped
//! binary has no way to do this. Shows only the synthetic profile it creates.

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use hatate_core::config::Settings;
use hatate_core::paths::Paths;
use hatate_gtk::{app, fonts, window::Env};
use std::cell::Cell;
use std::ffi::OsString;
use std::rc::Rc;

fn main() -> glib::ExitCode {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().expect("OUT.png"));
    let scheme = args.next().unwrap_or_else(|| "dark".into());
    let size = args.next();
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
    let mut settings = Settings::with_defaults(&paths);
    settings.theme = scheme;
    settings.save(&paths).expect("save settings");
    if let Err(e) = fonts::load(&home.path().join("fonts")) {
        eprintln!("fonts: {e}");
    }
    let env = Env::new(paths, false);
    let out2 = out.clone();
    let code = app::run_with(env, move |window| {
        if let Some((w, h)) = size.as_deref().and_then(|s| s.split_once('x')) {
            window
                .win
                .set_default_size(w.parse().unwrap(), h.parse().unwrap());
        }
        let win = window.win.clone();
        let frames = Rc::new(Cell::new(0u32));
        let out = out2.clone();
        win.add_tick_callback(move |win, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 12 {
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
            if let Some(app) = win.application() {
                app.quit();
            }
            glib::ControlFlow::Break
        });
    });
    drop(home);
    code
}
