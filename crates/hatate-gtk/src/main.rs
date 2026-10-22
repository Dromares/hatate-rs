//! Spike binary (DAN-1967). Work in progress, not yet a replacement for Hatate.

fn main() -> gtk4::glib::ExitCode {
    let start = std::time::Instant::now();
    match hatate_gtk::options::parse(std::env::args().skip(1)) {
        Ok(opts) => hatate_gtk::app::run(opts, start),
        Err(msg) => {
            eprintln!("{msg}");
            gtk4::glib::ExitCode::from(2)
        }
    }
}
