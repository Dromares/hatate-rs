//! The search engines a window test runs against: scripted answers instead of IQDB and SauceNAO,
//! a manual clock so a pacing wait costs nothing, and no network. Reuses the core crate's
//! `ScriptedEngines` (the same script the batch tests use).

#![allow(dead_code)]

#[path = "../../../hatate-core/tests/common/scripted.rs"]
pub mod scripted;

use hatate_core::clock::{Clock, ManualClock};
use hatate_core::engine_alerts::EngineAlerts;
use hatate_core::engine_runner::EngineRunner;
use hatate_core::engines::saucenao::SauceNaoQuota;
use hatate_core::net::{NetClient, NetConfig, install_crypto_provider};
use hatate_core::paths::Paths;
use hatate_core::search_engine::SearchEngine;
use hatate_gtk::search_run::SearchBackend;
use scripted::ScriptedEngines;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A 4 x 3 PNG (valid, 94 bytes): the "image" every searched row points at.
pub const PNG: [u8; 94] = [
    0x89, 0x50, 0x4e, 0x47, 0xd, 0xa, 0x1a, 0xa, 0x0, 0x0, 0x0, 0xd, 0x49, 0x48, 0x44, 0x52, 0x0,
    0x0, 0x0, 0x4, 0x0, 0x0, 0x0, 0x3, 0x8, 0x2, 0x0, 0x0, 0x0, 0x3b, 0x96, 0x39, 0x91, 0x0, 0x0,
    0x0, 0x25, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x5, 0xc1, 0x31, 0x1, 0x0, 0x0, 0xc, 0xc2, 0xb0,
    0x7e, 0xb5, 0x32, 0x11, 0x88, 0x40, 0xe, 0xf2, 0x97, 0x0, 0x1e, 0x16, 0x87, 0x10, 0x2f, 0x36,
    0x2e, 0xc2, 0xbc, 0xd9, 0xb9, 0xf9, 0x57, 0x51, 0x5, 0xf5, 0xb5, 0x5b, 0x46, 0x2a, 0x0, 0x0,
    0x0, 0x0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// What a test holds on to after the backend is handed to the window.
pub struct Scripted {
    pub engines: Arc<ScriptedEngines>,
    pub clock: Arc<ManualClock>,
    pub alerts: Arc<EngineAlerts>,
    /// The daily quota reads as exhausted once the engines have been called this many times.
    pub quota_after_calls: Arc<AtomicUsize>,
    pub last_quota: Arc<Mutex<Option<SauceNaoQuota>>>,
    pub resets: Arc<AtomicUsize>,
    pub down: Arc<AtomicBool>,
}

impl Scripted {
    pub fn new(paths: &Paths) -> (SearchBackend, Scripted) {
        install_crypto_provider();
        let clock = Arc::new(ManualClock::new(super::NOW, 0.0));
        let engines = Arc::new(ScriptedEngines::default());
        let runner = EngineRunner::new(engines.clone(), clock.clone());
        let net = NetClient::new(NetConfig::default()).expect("the net client");
        let engine = Arc::new(SearchEngine::with_recorder(
            net,
            runner,
            paths.clone(),
            clock.clone() as Arc<dyn Clock>,
            None,
        ));
        let alerts = Arc::new(EngineAlerts::new());
        let quota_after_calls = Arc::new(AtomicUsize::new(usize::MAX));
        let last_quota = Arc::new(Mutex::new(None));
        let resets = Arc::new(AtomicUsize::new(0));
        let down = Arc::new(AtomicBool::new(false));
        let (e, q) = (engines.clone(), quota_after_calls.clone());
        let lq = last_quota.clone();
        let (r, d) = (resets.clone(), down.clone());
        let backend = SearchBackend {
            engine,
            quota_exhausted: Arc::new(move || e.calls().len() >= q.load(Ordering::SeqCst)),
            last_quota: Arc::new(move || *lq.lock().unwrap()),
            reset_run_state: Arc::new(move || {
                r.fetch_add(1, Ordering::SeqCst);
            }),
            hydrus_client: Arc::new(|s| hatate_core::hydrus_client::HydrusClient::new(s)),
            alerts: alerts.clone(),
            unit_random: Arc::new(|| 0.5),
            shutdown: Arc::new(move || d.store(true, Ordering::SeqCst)),
        };
        (
            backend,
            Scripted {
                engines,
                clock,
                alerts,
                quota_after_calls,
                last_quota,
                resets,
                down,
            },
        )
    }
}
