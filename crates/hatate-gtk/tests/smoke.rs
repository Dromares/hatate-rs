//! Builds the real window (5,000-row Queue and the Review page), renders it to a PNG and
//! checks the file. Runs only when `HATATE_GTK_SMOKE=1` and a display is available.

use hatate_gtk::options::Options;
use std::time::Instant;

fn main() {
    if std::env::var("HATATE_GTK_SMOKE").as_deref() != Ok("1") {
        println!("hatate-gtk smoke: skipped (set HATATE_GTK_SMOKE=1 under a display)");
        return;
    }
    let dir = std::env::temp_dir().join(format!("hatate-gtk-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    for (name, tab, mode) in [("queue", "queue", "side"), ("wipe", "review", "wipe")] {
        let shot = dir.join(format!("{name}.png"));
        let opts = Options {
            tab: tab.into(),
            mode: hatate_gtk::review::Mode::parse(mode).expect("mode"),
            shot: Some(shot.clone()),
            ..Options::default()
        };
        // `run` owns the GTK main loop until the driver quits after the PNG is written.
        let code = hatate_gtk::app::run(opts, Instant::now());
        assert_eq!(
            code,
            gtk4::glib::ExitCode::SUCCESS,
            "{name}: app exited with failure"
        );
        let png = std::fs::read(&shot).unwrap_or_else(|e| panic!("{name}: no PNG: {e}"));
        assert!(png.starts_with(b"\x89PNG"), "{name}: not a PNG");
        assert!(
            png.len() > 5_000,
            "{name}: PNG suspiciously small ({} bytes)",
            png.len()
        );
    }
    std::fs::remove_dir_all(&dir).ok();
    println!("hatate-gtk smoke: ok");
}
