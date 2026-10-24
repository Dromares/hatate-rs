//! `hatate batch <dir>`: builds the shipped engines on the shipped net layer (rustls with the
//! explicit provider, the real hosts) and runs `hatate_core::batch::run_batch` on the profile in
//! the XDG directories. There is no flag or variable that points an engine or a site at another
//! host, and none that turns TLS verification off.

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hatate_core::batch::{BatchEnv, Ending, Reporter, run_batch};
use hatate_core::clock::{Clock, SystemClock};
use hatate_core::config::Settings;
use hatate_core::crash_marker::CrashMarkers;
use hatate_core::engine_backends::LiveEngines;
use hatate_core::engine_runner::EngineRunner;
use hatate_core::hydrus_client::HydrusClient;
use hatate_core::net::{NetClient, NetConfig, install_crypto_provider};
use hatate_core::paths::Paths;
use hatate_core::quota_pause::QuotaPauseFile;
use hatate_core::search_engine::SearchEngine;
use hatate_core::session::{LoadOptions, SessionStore};

use crate::args::BatchOptions;

/// The engines `hatate batch` searches with: the shipped ones, and the profile's own Pawchive
/// index behind `pawchive_index`. A function of its own so a test can pin that the index is wired.
fn build_engines(net: NetClient, clock: Arc<dyn Clock>, paths: &Paths) -> LiveEngines {
    LiveEngines::for_profile(net, clock, paths)
}

/// Progress on stderr: lines, and a countdown for the waits. On a terminal the countdown rewrites
/// one line; otherwise it prints when a wait starts and every 15 seconds after.
struct StderrReporter {
    tty: bool,
    /// The last whole-second value printed for the running wait, `None` between waits.
    last: Mutex<Option<i64>>,
}

impl StderrReporter {
    fn new() -> Self {
        StderrReporter {
            tty: std::io::stderr().is_terminal(),
            last: Mutex::new(None),
        }
    }
}

impl Reporter for StderrReporter {
    fn line(&self, text: &str) {
        if self.tty && self.last.lock().unwrap().take().is_some() {
            eprint!("\r{:60}\r", "");
        }
        eprintln!("{text}");
    }

    fn countdown(&self, label: &str, remaining: f64, total: f64) {
        let mut last = self.last.lock().unwrap();
        if label.is_empty() {
            if self.tty && last.take().is_some() {
                eprint!("\r{:60}\r", "");
            }
            *last = None;
            return;
        }
        let shown = remaining.round() as i64;
        if self.tty {
            eprint!("\r{label}: {shown}s of {total:.0}s    ");
            *last = Some(shown);
        } else if last.is_none() || shown % 15 == 0 && *last != Some(shown) {
            eprintln!("{label}: waiting {shown}s of {total:.0}s");
            *last = Some(shown);
        }
    }
}

/// `random.uniform`'s source: xorshift64*, seeded from the clock and the pid. Pacing jitter needs
/// no better than this, and a dependency for it would be a bigger thing than the use.
fn unit_random() -> impl Fn() -> f64 + Send + Sync {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9E37_79B9_7F4A_7C15, |d| d.as_nanos() as u64)
        ^ (u64::from(std::process::id()) << 32);
    let state = AtomicU64::new(seed | 1);
    move || {
        let mut x = state.load(Ordering::Relaxed);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        state.store(x, Ordering::Relaxed);
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn init_logging(verbosity: u8) {
    let level = match verbosity {
        0 => tracing::Level::WARN,
        1 => tracing::Level::INFO,
        _ => tracing::Level::DEBUG,
    };
    let _ = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .try_init();
}

/// Runs the command and returns the exit status.
pub fn run(opts: BatchOptions) -> i32 {
    init_logging(opts.verbosity);
    if !opts.dir.is_dir() {
        eprintln!("hatate: {} is not a folder", opts.dir.display());
        return 1;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("hatate: could not start the runtime: {e}");
            return 1;
        }
    };
    runtime.block_on(batch(opts))
}

/// The signal that stopped the run, if any (`0` while none has).
static SIGNAL: AtomicU64 = AtomicU64::new(0);

async fn batch(opts: BatchOptions) -> i32 {
    install_crypto_provider();
    let paths = match Paths::from_env() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("hatate: {e}");
            return 1;
        }
    };
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::default());
    let loaded = Settings::load(&paths);
    let settings = Mutex::new(loaded.settings);

    // Detects an interrupted earlier run, then marks this one as running.
    let started = CrashMarkers::begin(&paths, clock.wall());
    if let Some(e) = &started.marker_error {
        tracing::warn!("Could not write the crash marker: {e}");
    }

    let store = SessionStore::new(&paths);
    let writer = match store.open_writer() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("hatate: {e}");
            return 1;
        }
    };
    let restored = writer.restore(&LoadOptions {
        now: clock.wall(),
        unclean_shutdown: started.previous_run_unclean,
    });
    let mut entries = restored.entries;
    if !entries.is_empty() {
        eprintln!(
            "hatate: continuing the saved session ({} entries)",
            entries.len()
        );
    }

    let net = match NetClient::new(NetConfig::default()) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("hatate: could not build the network client: {e}");
            return 1;
        }
    };
    let live = Arc::new(build_engines(net.clone(), clock.clone(), &paths));
    let runner = EngineRunner::new(live.clone(), clock.clone());
    let engine = SearchEngine::new(net, runner, paths.clone(), clock.clone());

    let stop = Arc::new(AtomicBool::new(false));
    spawn_signal_handler(stop.clone());

    let quota_file = QuotaPauseFile::new(&paths);
    let reporter: Arc<dyn Reporter> = Arc::new(StderrReporter::new());
    let random = unit_random();
    let quota_live = live.clone();
    let reset_live = live.clone();
    let quota = move || quota_live.saucenao.is_daily_quota_exhausted();
    let reset = move || reset_live.reset_run_state();
    let hydrus = |settings: &hatate_core::config::HydrusSettings| HydrusClient::new(settings);
    let env = BatchEnv {
        engine: &engine,
        settings: &settings,
        stop: stop.clone(),
        quota_exhausted: &quota,
        quota_file: &quota_file,
        sink: &writer,
        reporter: reporter.clone(),
        unit_random: &random,
        reset_run_state: &reset,
        hydrus_client: &hydrus,
    };

    let outcome = run_batch(&env, &opts.dir, &mut entries).await;
    live.shutdown();

    // The session is saved by now; the clean-exit marker says so.
    if let Err(e) = started.session.finish_clean(clock.wall()) {
        tracing::warn!("Could not record the clean exit: {e}");
    }
    eprintln!(
        "hatate: {} found, {} added, {} duplicate(s), {} search(es), {} entries in the session",
        outcome.found,
        outcome.added,
        outcome.duplicates,
        outcome.searched,
        entries.len()
    );
    let signal = SIGNAL.load(Ordering::SeqCst);
    if outcome.save_failed {
        eprintln!("hatate: the session could not be saved");
        return 1;
    }
    match outcome.ending {
        Ending::Finished => 0,
        Ending::PausedForQuota { .. } => 3,
        Ending::Failed(fault) => {
            eprintln!(
                "hatate: the search run stopped on an error: {}",
                fault.user_message()
            );
            1
        }
        Ending::Stopped if signal == 15 => 143,
        Ending::Stopped => 130,
    }
}

/// SIGINT and SIGTERM raise the stop flag once; the loop saves the session and exits. A second
/// signal gives up on the save and exits at once.
fn spawn_signal_handler(stop: Arc<AtomicBool>) {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut int), Ok(mut term)) = (
        signal(SignalKind::interrupt()),
        signal(SignalKind::terminate()),
    ) else {
        tracing::warn!("Could not install the signal handlers; Ctrl-C will not save the session");
        return;
    };
    tokio::spawn(async move {
        loop {
            let which = tokio::select! {
                _ = int.recv() => 2,
                _ = term.recv() => 15,
            };
            if stop.swap(true, Ordering::SeqCst) {
                // The staging directory of a run in progress goes too; the next run's sweep would
                // find it otherwise.
                hatate_core::batch::cleanup_current_staging();
                eprintln!("hatate: stopping at once, the session was not saved");
                std::process::exit(128 + which as i32);
            }
            SIGNAL.store(which, Ordering::SeqCst);
            eprintln!("hatate: stopping; saving the session (signal again to quit at once)");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use hatate_core::clock::ManualClock;
    use hatate_core::engine_runner::{Engines, IndexAnswer};
    use hatate_core::image_compare::Source;
    use hatate_core::pawchive_index::{ImageRow, PawchiveIndex, fingerprints};

    fn picture() -> Vec<u8> {
        let mut img = image::RgbImage::new(64, 64);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 4) as u8, (y * 4) as u8, ((x * y) % 251) as u8]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// `hatate batch` searches the Pawchive index in the profile, not an empty one: the engines
    /// `build_engines` returns answer `Matches` for a picture the profile's index holds.
    #[tokio::test]
    async fn the_batch_engines_search_the_profiles_pawchive_index() {
        install_crypto_provider();
        let home = tempfile::tempdir().unwrap();
        let home_os = home.path().as_os_str().to_owned();
        let paths = Paths::from_lookup(|k| (k == "HOME").then(|| home_os.clone())).unwrap();

        let bytes = picture();
        let (dhash8, dhash16) = fingerprints(Source::Bytes(&bytes)).unwrap();
        let hex = |h: hatate_core::image_compare::Hash| {
            format!(
                "{:016x}{:016x}{:016x}{:016x}",
                h.0[0], h.0[1], h.0[2], h.0[3]
            )
        };
        PawchiveIndex::new(paths.pawchive_index_db())
            .add_images(&[ImageRow {
                service: "fanbox".to_owned(),
                user_id: "artist-7".to_owned(),
                post_id: "120".to_owned(),
                page_index: 0,
                sha256: "0".repeat(64),
                path: "/00/00/picture.png".to_owned(),
                title: Some("a post".to_owned()),
                dhash8: dhash8.map(|h| format!("{:016x}", h.0[3])),
                dhash16: dhash16.map(hex),
            }])
            .unwrap();
        let local = home.path().join("local.png");
        std::fs::write(&local, &bytes).unwrap();

        let net = NetClient::new(NetConfig::default()).unwrap();
        let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(1_700_000_000.0, 100.0));
        let engines = build_engines(net, clock, &paths);
        let answer = engines.pawchive_index(Path::new(&local), None).await;
        assert!(
            matches!(&answer, Ok(IndexAnswer::Matches(m)) if m.len() == 1),
            "{answer:?}"
        );
    }
}
