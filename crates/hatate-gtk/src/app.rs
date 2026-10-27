//! The application: `AdwApplication`, one window, and what happens around it (`main.py` at
//! `76c85c4`).
//!
//! [`run_with`] is the whole startup for a caller that holds an [`Env`]; the binary builds the env
//! from the process environment and passes a hook that does nothing. Tests and the shot renderer
//! pass a hook that drives the window. There is no flag, variable or config key that does it for
//! them.

use crate::window::{Env, MainWindow};
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;

/// The application id (D-Bus name and desktop-file stem).
pub const APP_ID: &str = "io.github.dromares.HatateRs";

/// Runs the application with `env`. `on_ready` is called once, with the window, after it is
/// presented. Returns when the application quits.
pub fn run_with(env: Env, on_ready: impl Fn(&MainWindow) + 'static) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let env = Rc::new(RefCell::new(Some(env)));
    app.connect_activate(move |app| {
        let Some(env) = env.borrow_mut().take() else {
            // A second activation (a second launch) raises the window that is already there.
            if let Some(w) = app.active_window() {
                w.present();
            }
            return;
        };
        let window = MainWindow::new(app, env);
        window.win.present();
        on_ready(&window);
        // The window lives as long as the application holds it.
        std::mem::forget(window);
    });
    app.run_with_args::<&str>(&[])
}

/// Starts the application and hands it to `f` once it is active, then returns when `f` (or
/// anything else) has quit it. For tests and the shot renderer, which build windows themselves.
pub fn with_application(f: impl FnOnce(&adw::Application) + 'static) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let f = Rc::new(RefCell::new(Some(f)));
    app.connect_activate(move |app| {
        if let Some(f) = f.borrow_mut().take() {
            f(app);
        }
    });
    app.run_with_args::<&str>(&[])
}
