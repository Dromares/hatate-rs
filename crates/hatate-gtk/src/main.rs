//! The Hatate window. Work in progress, not yet a replacement for Hatate.

use hatate_core::crash_marker::CrashMarkers;
use hatate_core::paths::Paths;
use hatate_gtk::{app, fonts, window::Env};

fn main() -> gtk4::glib::ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("hatate-gtk: {e}");
            return gtk4::glib::ExitCode::FAILURE;
        }
    };
    // Checked before this run's own marker goes down (`check_unclean_shutdown`, then
    // `mark_running`): still finding the previous one means that run never reached a clean exit.
    let started = CrashMarkers::begin(&paths, hatate_core::session::now_epoch());
    if let Some(e) = &started.marker_error {
        tracing::warn!("could not write the running marker: {e}");
    }
    // Before GTK starts: fontconfig reads its configuration once, at first use. A failure leaves
    // the window on fallback faces, as a font that fails to register does in Python.
    if let Err(e) = fonts::load_default() {
        tracing::warn!("fonts not loaded: {e}");
    }
    let env = Env::new(paths, started.previous_run_unclean);
    app::run_with(env, |_| {})
}
