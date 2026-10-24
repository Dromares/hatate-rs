//! The engines the runner asks in the shipped build ([`LiveEngines`]): the Phase 3 clients, plus
//! the two engines that are not ported yet, which answer as Python does when the engine cannot run
//! (DAN-2063).
//!
//! * **Google Lens** is the 5.a client over a [`LensBrowser`] ([`LiveEngines::for_profile`], which
//!   `hatate batch` builds: Lens's own profile under the config directory, the real Google, the
//!   rule34.xxx and Reddit lookups on the net layer). With Lens off in the config the engine is
//!   never asked. With no Chromium to start, the browser answers "unavailable" before any page is
//!   touched, and the engine latches for the run as in Python (`_latch`): the next image finds it
//!   blocked and the row says "not searched (it stood down earlier this run)". A
//!   [`LiveEngines::new`] has no browser at all ([`LENS_UNAVAILABLE_MESSAGE`]), which is what every
//!   test that does not mean to start one gets.
//! * **The Pawchive index** is the 4.f [`PawchiveIndex`] over `pawchive_index.db`, as
//!   `_collect_pawchive_index` reads it: `is_empty` first (so an empty or missing index costs no
//!   file read), then `fingerprints(path)`, then `search(hash16, hydrus_hash)`. It is local and
//!   read-only here: the search opens the file the way Python's `_connect` does (the schema's
//!   `IF NOT EXISTS` statements, which write nothing to an index that already has its tables) and
//!   never adds a row. A [`LiveEngines`] built without [`LiveEngines::with_pawchive_index`] has no
//!   index and answers as an empty one.

use std::path::Path;
use std::sync::Arc;

use crate::clock::{BoxFuture, Clock};
use crate::config::{SauceNaoSettings, Secret};
use crate::engine_runner::{Engines, IndexAnswer, IndexError, IndexMatch, LensError};
use crate::engines::ascii2d::{Ascii2dClient, Ascii2dError, Ascii2dMatch};
use crate::engines::client::OnTick;
use crate::engines::google_images::{GoogleImagesClient, GoogleImagesError, GoogleImagesMatch};
use crate::engines::google_lens::GoogleLensMatch;
use crate::engines::iqdb::{IqdbClient, IqdbError, IqdbMatch};
use crate::engines::lens_rank::LensSites;
pub use crate::engines::lens_search::LENS_UNAVAILABLE_MESSAGE;
use crate::engines::lens_search::LensClient;
use crate::engines::lens_sites::OnlineSites;
use crate::engines::pawchive_lookup::{
    PawchiveError, PawchiveLookupClient, PawchiveMatch, file_sha256,
};
use crate::engines::saucenao::{SauceNaoClient, SauceNaoError, SauceNaoMatch, ShouldStop};
use crate::engines::tracemoe::{TraceMoeClient, TraceMoeError, TraceMoeMatch};
use crate::engines::yandex::{YandexClient, YandexError, YandexMatch};
use crate::image_compare::Source;
use crate::image_prep::os_error_text;
use crate::lens_browser::LensBrowser;
use crate::net::NetClient;
use crate::paths::Paths;
use crate::pawchive_index::{self, PawchiveIndex};

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
    pub lens: LensClient,
    /// The browser behind `lens`, when it has one: stopped by [`LiveEngines::shutdown`].
    lens_browser: Option<LensBrowser>,
    /// `engine_runner._pawchive_index`: one index for every search, so its fingerprints are
    /// loaded once and reloaded only after the index changes.
    pawchive_index: Option<Arc<PawchiveIndex>>,
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
            lens: LensClient::new().with_clock(clock),
            lens_browser: None,
            pawchive_index: None,
        }
    }

    /// What `hatate batch` builds: [`LiveEngines::new`] with the profile's own Pawchive index.
    pub fn for_profile(net: NetClient, clock: Arc<dyn Clock>, paths: &Paths) -> Self {
        let sites = Arc::new(OnlineSites::new(net.clone()));
        Self::new(net, clock)
            .with_lens_browser(LensBrowser::new(paths.lens_profile_dir()), sites)
            .with_pawchive_index(PawchiveIndex::new(paths.pawchive_index_db()))
    }

    /// Lens over `browser`, with `sites` for its recovery lookups.
    #[must_use]
    pub fn with_lens_browser(mut self, browser: LensBrowser, sites: Arc<dyn LensSites>) -> Self {
        self.lens = LensClient::with_fetch(Arc::new(browser.clone()))
            .with_clock(self.lens_clock())
            .with_sites(sites);
        self.lens_browser = Some(browser);
        self
    }

    /// The clock the Lens client was built on.
    fn lens_clock(&self) -> Arc<dyn Clock> {
        self.lens.clock()
    }

    /// Stops the Lens browser, if there is one (`lens_browser.shutdown()`, which the GUI calls on
    /// the way out). The browser dies with the process anyway; this lets it close its window.
    pub fn shutdown(&self) {
        if let Some(browser) = &self.lens_browser {
            browser.shutdown();
        }
    }

    /// Searches `index` for the Pawchive index engine.
    #[must_use]
    pub fn with_pawchive_index(mut self, index: PawchiveIndex) -> Self {
        self.pawchive_index = Some(Arc::new(index));
        self
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
        self.lens.is_resting()
    }

    fn google_lens_resting_until(&self) -> String {
        self.lens.resting_until()
    }

    fn google_lens_wait_for_turn<'a>(&'a self, on_tick: Option<OnTick>) -> BoxFuture<'a, ()> {
        Box::pin(async move { self.lens.wait_for_turn(on_tick, None).await })
    }

    fn google_lens<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        whole_image: bool,
    ) -> BoxFuture<'a, Result<Vec<GoogleLensMatch>, LensError>> {
        Box::pin(async move { self.lens.search(path, timeout, on_tick, whole_image).await })
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
        path: &'a Path,
        hydrus_hash: Option<&'a str>,
    ) -> BoxFuture<'a, Result<IndexAnswer, IndexError>> {
        let Some(index) = self.pawchive_index.clone() else {
            return Box::pin(std::future::ready(Ok(IndexAnswer::Empty)));
        };
        let path = path.to_owned();
        let hydrus_hash = hydrus_hash.map(str::to_owned);
        Box::pin(async move {
            match tokio::task::spawn_blocking(move || {
                search_index(&index, &path, hydrus_hash.as_deref())
            })
            .await
            {
                Ok(answer) => answer,
                Err(join) => Err(IndexError(join.to_string())),
            }
        })
    }
}

/// The body of `_collect_pawchive_index` up to the candidates. Python catches only
/// `sqlite3.Error`; here every failure of the index is an [`IndexError`] with its message (an
/// unreadable folder or a malformed stored hash would escape as another exception there). A
/// picture past Pillow's decompression-bomb limit cannot reach this point in a search: the entry's
/// size is read first and that raises, so the message here is only a floor.
fn search_index(
    index: &PawchiveIndex,
    path: &Path,
    hydrus_hash: Option<&str>,
) -> Result<IndexAnswer, IndexError> {
    let failed = |e: pawchive_index::PawchiveError| IndexError(e.message().to_owned());
    if index.is_empty().map_err(failed)? {
        return Ok(IndexAnswer::Empty);
    }
    let (_, hash16) = pawchive_index::fingerprints(Source::Path(path))
        .map_err(|bomb| IndexError(bomb.to_string()))?;
    let Some(hash16) = hash16 else {
        return Ok(IndexAnswer::Unreadable);
    };
    let matches = index
        .search_default(Some(&hash16), hydrus_hash)
        .map_err(failed)?;
    Ok(IndexAnswer::Matches(
        matches
            .into_iter()
            .map(|m| IndexMatch {
                url: m.url,
                thumb_url: Some(m.thumb_url),
                similarity: m.similarity,
                exact: m.exact,
            })
            .collect(),
    ))
}
