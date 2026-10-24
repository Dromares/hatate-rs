//! The Hatate window. Work in progress, not yet a replacement for Hatate.

use hatate_core::crash_marker::CrashMarkers;
use hatate_core::paths::Paths;
use hatate_core::rotating_log::RotatingLog;
use hatate_gtk::logsink::FileLayer;
use hatate_gtk::{app, fonts, window::Env};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn main() -> gtk4::glib::ExitCode {
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("hatate-gtk: {e}");
            return gtk4::glib::ExitCode::FAILURE;
        }
    };
    // stderr as before, and `hatate-rs.log` (the file the log viewer reads). A profile that
    // cannot be written to leaves the app on stderr alone.
    let file = RotatingLog::start(&paths, hatate_core::session::now_epoch()).ok();
    let file_failed = file.is_none();
    tracing_subscriber::registry()
        .with(LevelFilter::INFO)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false),
        )
        .with(file.map(FileLayer::new))
        .init();
    if file_failed {
        tracing::warn!("could not open hatate-rs.log; logging to stderr only");
    }
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
