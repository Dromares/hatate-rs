//! Spike binary (DAN-1967, DAN-1983). Work in progress, not yet a replacement for Hatate.

use gtk4::glib::ExitCode;

fn main() -> ExitCode {
    let opts = match hatate_gtk::options::parse(std::env::args().skip(1)) {
        Ok(opts) => opts,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };

    // Fonts first: fontconfig reads its configuration once, at first use, so the run-time font
    // directory has to be handed over before GTK starts. A failure is reported and the run
    // continues on fallback faces (the report then says so).
    if let Some(dir) = &opts.fonts {
        let t = std::time::Instant::now();
        match hatate_gtk::fonts::load(dir, opts.font_route) {
            Ok(()) => println!(
                "FONT load route={:?} dir={} files={} took_ms={:.1}",
                opts.font_route,
                dir.display(),
                hatate_gtk::fonts::list_font_files(dir).len(),
                t.elapsed().as_secs_f64() * 1e3
            ),
            Err(e) => println!("FONT load failed route={:?}: {e}", opts.font_route),
        }
    }

    // Thumbnail sources are generated before the clock starts: they stand in for the user's
    // images, which already exist when the app is opened.
    if let Some(dir) = &opts.thumbs {
        let t = std::time::Instant::now();
        match hatate_gtk::thumbs::generate(dir, opts.rows) {
            Ok(n) => println!(
                "THUMBS generated={n} of {} under {} in {:.0} ms",
                opts.rows,
                dir.display(),
                t.elapsed().as_secs_f64() * 1e3
            ),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        }
    }

    let start = std::time::Instant::now();
    hatate_gtk::app::run(opts, start)
}
