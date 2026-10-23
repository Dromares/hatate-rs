//! The engines the runner asks in the shipped build ([`LiveEngines`]): the Phase 3 clients, plus
//! the two engines that are not ported yet, which answer as Python does when the engine cannot run
//! (DAN-2063).
//!
//! * **Google Lens** (Phase 5) copies `google_lens.search`'s unavailable path: it reads the file
//!   (`Could not read image: ...` if it cannot), then raises `GoogleLensUnavailableError`, which
//!   latches the engine (`_latch`). The next image finds it blocked and the row says "not searched
//!   (it stood down earlier this run)". The pacing `wait_for_turn` runs before every search that
//!   is not blocked, as in Python, so the 45 s spacing is requested of the clock. The message is
//!   [`LENS_UNAVAILABLE_MESSAGE`]: in Python it is the text of the missing-dependency error for
//!   the interpreter, which has no counterpart here. Not copied: `announce_unavailable` (the
//!   once-per-session GUI alert) and `prepare_for_lens`'s format check, so a readable file the
//!   Python app would refuse as "not in a format Google Lens accepts" gets the unavailable answer
//!   instead.
//! * **The Pawchive index** (Phase 4) answers as an index with nothing in it: `index.is_empty()` is
//!   true, so the collector returns False with no error and no file read.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::clock::{BoxFuture, Clock};
use crate::config::{SauceNaoSettings, Secret};
use crate::engine_runner::{Engines, IndexAnswer, IndexError, LensError};
use crate::engines::ascii2d::{Ascii2dClient, Ascii2dError, Ascii2dMatch};
use crate::engines::client::OnTick;
use crate::engines::google_images::{GoogleImagesClient, GoogleImagesError, GoogleImagesMatch};
use crate::engines::google_lens::GoogleLensMatch;
use crate::engines::iqdb::{IqdbClient, IqdbError, IqdbMatch};
use crate::engines::pawchive_lookup::{
    PawchiveError, PawchiveLookupClient, PawchiveMatch, file_sha256,
};
use crate::engines::saucenao::{SauceNaoClient, SauceNaoError, SauceNaoMatch, ShouldStop};
use crate::engines::tracemoe::{TraceMoeClient, TraceMoeError, TraceMoeMatch};
use crate::engines::yandex::{YandexClient, YandexError, YandexMatch};
use crate::image_prep::os_error_text;
use crate::net::NetClient;

/// What Google Lens says when it cannot run. See the module comment.
pub const LENS_UNAVAILABLE_MESSAGE: &str =
    "No browser engine is available to render the results page in this build.";

/// `google_lens.MIN_SECONDS_BETWEEN_SEARCHES`.
const LENS_MIN_SECONDS_BETWEEN_SEARCHES: f64 = 45.0;

#[derive(Default)]
struct LensState {
    blocked: bool,
    next_turn: Option<f64>,
}

/// Google Lens in a build that cannot run it: the state `google_lens.py` keeps in globals, and
/// the unavailable path of `search`.
pub struct LensStandIn {
    clock: Arc<dyn Clock>,
    state: Mutex<LensState>,
}

impl LensStandIn {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        LensStandIn {
            clock,
            state: Mutex::default(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LensState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `is_blocked` (it never rests: nothing here can be challenged).
    pub fn is_blocked(&self) -> bool {
        self.lock().blocked
    }

    /// `reset_blocked_flag`: called whenever a search starts.
    pub fn reset_blocked_flag(&self) {
        self.lock().blocked = false;
    }

    /// `wait_for_turn(on_tick)`: holds a search until 45 s have passed since the last one started.
    pub async fn wait_for_turn(&self, on_tick: Option<OnTick>) {
        let now = self.clock.monotonic();
        let start_at = {
            let mut state = self.lock();
            let start_at = now.max(state.next_turn.filter(|t| *t != 0.0).unwrap_or(now));
            state.next_turn = Some(start_at + LENS_MIN_SECONDS_BETWEEN_SEARCHES);
            start_at
        };
        let wait = start_at - now;
        if wait <= 0.0 {
            return;
        }
        tracing::info!("Pacing Google Lens: waiting {wait:.0}s so Google isn't asked too often");
        loop {
            let left = start_at - self.clock.monotonic();
            if left <= 0.0 {
                return;
            }
            if let Some(tick) = &on_tick {
                tick("Pacing Google Lens", left, wait);
            }
            self.clock.sleep(left.min(1.0)).await;
        }
    }

    /// `search`, as far as it can go: reading the file, then the unavailable error and its latch.
    pub fn search(&self, image_path: &Path) -> Result<Vec<GoogleLensMatch>, LensError> {
        if let Err(exc) = std::fs::File::open(image_path) {
            tracing::error!(
                "Could not read {} for the Lens upload: {exc}",
                image_path.display()
            );
            return Err(LensError::Failed(format!(
                "Could not read image: {}",
                os_error_text(&exc, image_path)
            )));
        }
        self.lock().blocked = true;
        Err(LensError::Unavailable(LENS_UNAVAILABLE_MESSAGE.to_owned()))
    }
}

/// The shipped engines. The clients are public so a test can build them against a local https
/// server (their `with_*` constructors) and the caller can reach their state (`reset_blocked_flag`
/// and the like).
pub struct LiveEngines {
    pub iqdb: IqdbClient,
    pub saucenao: SauceNaoClient,
    pub ascii2d: Ascii2dClient,
    pub tracemoe: TraceMoeClient,
    pub google_images: GoogleImagesClient,
    pub yandex: YandexClient,
    pub pawchive: PawchiveLookupClient,
    pub lens: LensStandIn,
}

impl LiveEngines {
    /// The real hosts, on `net` (the 1.c net layer: rustls with the explicit provider), with the
    /// waits and paces measured on `clock`.
    pub fn new(net: NetClient, clock: Arc<dyn Clock>) -> Self {
        LiveEngines {
            iqdb: IqdbClient::new(net.clone()),
            saucenao: SauceNaoClient::new(net.clone()).with_clock(Arc::clone(&clock)),
            ascii2d: Ascii2dClient::new(net.clone()),
            tracemoe: TraceMoeClient::new(net.clone()),
            google_images: GoogleImagesClient::new(net.clone()),
            yandex: YandexClient::new(net.clone()).with_clock(Arc::clone(&clock)),
            pawchive: PawchiveLookupClient::new(net),
            lens: LensStandIn::new(clock),
        }
    }
}

impl LiveEngines {
    /// What `_launch_search_worker` does before a run: forget the remembered SauceNAO quota
    /// exhaustion, and every engine that stood down on an earlier run (ascii2d, Google, Lens,
    /// Yandex), since a site's bot check comes and goes.
    pub fn reset_run_state(&self) {
        self.saucenao.reset_daily_limit_flag();
        self.ascii2d.reset_blocked_flag();
        self.google_images.reset_blocked_flag();
        self.lens.reset_blocked_flag();
        self.yandex.reset_blocked_flag();
    }
}

impl Engines for LiveEngines {
    fn iqdb<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        three_d: bool,
    ) -> BoxFuture<'a, Result<Vec<IqdbMatch>, IqdbError>> {
        Box::pin(async move {
            if three_d {
                self.iqdb.search_3d(path, timeout, on_tick).await
            } else {
                self.iqdb.search(path, timeout, on_tick).await
            }
        })
    }

    fn saucenao<'a>(
        &'a self,
        path: &'a Path,
        settings: &'a SauceNaoSettings,
        timeout: f64,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> BoxFuture<'a, Result<Vec<SauceNaoMatch>, SauceNaoError>> {
        Box::pin(async move {
            self.saucenao
                .search(path, settings, timeout, on_tick, should_stop)
                .await
        })
    }

    fn ascii2d_is_blocked(&self) -> bool {
        self.ascii2d.is_blocked()
    }

    fn ascii2d<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
    ) -> BoxFuture<'a, Result<Vec<Ascii2dMatch>, Ascii2dError>> {
        Box::pin(async move { self.ascii2d.search(path, timeout, on_tick).await })
    }

    fn tracemoe<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        min_similarity: f64,
    ) -> BoxFuture<'a, Result<Vec<TraceMoeMatch>, TraceMoeError>> {
        Box::pin(async move {
            self.tracemoe
                .search(path, timeout, on_tick, min_similarity)
                .await
        })
    }

    fn google_images_is_blocked(&self) -> bool {
        self.google_images.is_blocked()
    }

    fn google_images<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        api_key: &'a Secret,
    ) -> BoxFuture<'a, Result<Vec<GoogleImagesMatch>, GoogleImagesError>> {
        Box::pin(async move {
            // The config's secret and the net layer's are two types; the key is copied across
            // for the one call and is redacted in `Debug` on both sides.
            let key = crate::net::Secret::new(api_key.expose());
            self.google_images
                .search(path, timeout, on_tick, &key)
                .await
        })
    }

    fn google_lens_is_blocked(&self) -> bool {
        self.lens.is_blocked()
    }

    fn google_lens_is_resting(&self) -> bool {
        false
    }

    fn google_lens_resting_until(&self) -> String {
        String::new()
    }

    fn google_lens_wait_for_turn<'a>(&'a self, on_tick: Option<OnTick>) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.lens.wait_for_turn(on_tick).await })
    }

    fn google_lens<'a>(
        &'a self,
        path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        _whole_image: bool,
    ) -> BoxFuture<'a, Result<Vec<GoogleLensMatch>, LensError>> {
        Box::pin(std::future::ready(self.lens.search(path)))
    }

    fn yandex_is_blocked(&self) -> bool {
        self.yandex.is_blocked()
    }

    fn yandex_is_resting(&self) -> bool {
        self.yandex.is_resting()
    }

    fn yandex_resting_until(&self) -> String {
        self.yandex.resting_until()
    }

    fn yandex_wait_for_turn<'a>(
        &'a self,
        on_tick: Option<OnTick>,
        should_stop: Option<ShouldStop>,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.yandex.wait_for_turn(on_tick, should_stop).await })
    }

    fn yandex<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
    ) -> BoxFuture<'a, Result<Vec<YandexMatch>, YandexError>> {
        Box::pin(async move { self.yandex.search(path, timeout, on_tick).await })
    }

    fn file_sha256<'a>(&'a self, path: &'a Path) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let owned = path.to_owned();
            match tokio::task::spawn_blocking(move || file_sha256(&owned)).await {
                Ok(Ok(hash)) => Ok(hash),
                Ok(Err(exc)) => Err(os_error_text(&exc, path)),
                Err(join) => Err(join.to_string()),
            }
        })
    }

    fn pawchive_lookup<'a>(
        &'a self,
        sha256: &'a str,
        timeout: f64,
    ) -> BoxFuture<'a, Result<Vec<PawchiveMatch>, PawchiveError>> {
        Box::pin(async move { self.pawchive.lookup(sha256, timeout).await })
    }

    fn pawchive_index<'a>(
        &'a self,
        _path: &'a Path,
        _hydrus_hash: Option<&'a str>,
    ) -> BoxFuture<'a, Result<IndexAnswer, IndexError>> {
        Box::pin(std::future::ready(Ok(IndexAnswer::Empty)))
    }
}
