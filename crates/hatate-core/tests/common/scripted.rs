//! Engines that answer from a script (DAN-2064), for tests of `search_image`. The runner calls
//! engines through the [`Engines`] trait, so a test says what each engine returns instead of
//! standing a server up for it. Engines a test does not script answer with no matches; Google
//! Images, Lens, Yandex and Pawchive are not scripted at all (a test that enables one gets a panic
//! that names it).

#![allow(dead_code)]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hatate_core::clock::BoxFuture;
use hatate_core::config::{SauceNaoSettings, Secret};
use hatate_core::engine_runner::{Engines, IndexAnswer, IndexError, LensError};
use hatate_core::engines::ascii2d::{Ascii2dError, Ascii2dMatch};
use hatate_core::engines::client::OnTick;
use hatate_core::engines::google_images::{GoogleImagesError, GoogleImagesMatch};
use hatate_core::engines::google_lens::GoogleLensMatch;
use hatate_core::engines::iqdb::{IqdbError, IqdbMatch};
use hatate_core::engines::pawchive_lookup::{PawchiveError, PawchiveMatch};
use hatate_core::engines::saucenao::{SauceNaoError, SauceNaoMatch, ShouldStop};
use hatate_core::engines::tracemoe::{TraceMoeError, TraceMoeMatch};
use hatate_core::engines::yandex::{YandexError, YandexMatch};
use hatate_core::model::{Tag, TagSource};

/// What one call to a scripted engine does.
pub enum Step<T> {
    Return(T),
    /// Waits first, so engines of one wave finish in a known order.
    After(Duration, T),
    /// An exception that is not the engine's own error type.
    Panic(&'static str),
}

pub struct Queue<T>(Mutex<VecDeque<Step<T>>>);

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Queue(Mutex::new(VecDeque::new()))
    }
}

impl<T> Queue<T> {
    pub fn push(&self, step: Step<T>) {
        self.0.lock().unwrap().push_back(step);
    }

    async fn take(&self, empty: impl FnOnce() -> T) -> T {
        let step = self.0.lock().unwrap().pop_front();
        match step {
            None => empty(),
            Some(Step::Return(v)) => v,
            Some(Step::After(wait, v)) => {
                tokio::time::sleep(wait).await;
                v
            }
            Some(Step::Panic(message)) => panic!("{message}"),
        }
    }
}

#[derive(Default)]
pub struct ScriptedEngines {
    pub iqdb: Queue<Result<Vec<IqdbMatch>, IqdbError>>,
    pub saucenao: Queue<Result<Vec<SauceNaoMatch>, SauceNaoError>>,
    pub ascii2d: Queue<Result<Vec<Ascii2dMatch>, Ascii2dError>>,
    pub tracemoe: Queue<Result<Vec<TraceMoeMatch>, TraceMoeError>>,
    pub ascii2d_blocked: AtomicBool,
    /// The engines to ask for the Pawchive index (DAN-2145: `LiveEngines` over a profile's real
    /// index file). Unset, the index answers as an empty one.
    pub index_backend: Mutex<Option<Arc<dyn Engines>>>,
    /// The engines to ask for Google Lens (DAN-2165: `LiveEngines` over a scripted `LensFetch`).
    /// Unset, a test that enables Lens gets a panic.
    pub lens_backend: Mutex<Option<Arc<dyn Engines>>>,
    calls: Mutex<Vec<&'static str>>,
}

impl ScriptedEngines {
    /// The engines asked, in the order they were asked (within a wave that is the order they
    /// started, which a test fixes with `Step::After`).
    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().unwrap().clone()
    }

    fn note(&self, name: &'static str) {
        self.calls.lock().unwrap().push(name);
    }
}

pub fn engine_tag(name: &str, namespace: Option<&str>) -> Tag {
    Tag {
        name: name.to_owned(),
        namespace: namespace.map(str::to_owned),
        source: TagSource::SearchEngine,
    }
}

pub fn iqdb_match(url: &str, similarity: f64) -> IqdbMatch {
    IqdbMatch {
        url: url.to_owned(),
        thumb_url: None,
        similarity,
        width: None,
        height: None,
        source_name: hatate_core::sites::guess_site_name(url).map(str::to_owned),
        is_best_match: false,
        unnamespaced_tags: Vec::new(),
    }
}

pub fn saucenao_match(url: &str, similarity: f64) -> SauceNaoMatch {
    SauceNaoMatch {
        url: url.to_owned(),
        similarity,
        title: None,
        source_name: hatate_core::sites::guess_site_name(url).map(str::to_owned),
        thumb_url: None,
        tags: Vec::new(),
    }
}

pub fn ascii2d_match(url: &str, similarity: f64) -> Ascii2dMatch {
    Ascii2dMatch {
        url: url.to_owned(),
        thumb_url: None,
        similarity,
        width: None,
        height: None,
        source_name: hatate_core::sites::guess_site_name(url).map(str::to_owned),
        search_kind: "color".to_owned(),
    }
}

impl Engines for ScriptedEngines {
    fn iqdb<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        three_d: bool,
    ) -> BoxFuture<'a, Result<Vec<IqdbMatch>, IqdbError>> {
        assert!(!three_d, "IQDB 3D is not scripted");
        Box::pin(async move {
            self.note("iqdb");
            self.iqdb.take(|| Ok(Vec::new())).await
        })
    }

    fn saucenao<'a>(
        &'a self,
        _path: &'a Path,
        _settings: &'a SauceNaoSettings,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        _should_stop: Option<ShouldStop>,
    ) -> BoxFuture<'a, Result<Vec<SauceNaoMatch>, SauceNaoError>> {
        Box::pin(async move {
            self.note("saucenao");
            self.saucenao.take(|| Ok(Vec::new())).await
        })
    }

    fn ascii2d_is_blocked(&self) -> bool {
        self.ascii2d_blocked.load(Ordering::SeqCst)
    }

    fn ascii2d<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
    ) -> BoxFuture<'a, Result<Vec<Ascii2dMatch>, Ascii2dError>> {
        Box::pin(async move {
            self.note("ascii2d");
            let answer = self.ascii2d.take(|| Ok(Vec::new())).await;
            if matches!(answer, Err(Ascii2dError::Blocked)) {
                self.ascii2d_blocked.store(true, Ordering::SeqCst);
            }
            answer
        })
    }

    fn tracemoe<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        _min_similarity: f64,
    ) -> BoxFuture<'a, Result<Vec<TraceMoeMatch>, TraceMoeError>> {
        Box::pin(async move {
            self.note("tracemoe");
            self.tracemoe.take(|| Ok(Vec::new())).await
        })
    }

    fn google_images_is_blocked(&self) -> bool {
        false
    }

    fn google_images<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
        _api_key: &'a Secret,
    ) -> BoxFuture<'a, Result<Vec<GoogleImagesMatch>, GoogleImagesError>> {
        panic!("Google Images is not scripted")
    }

    fn google_lens_is_blocked(&self) -> bool {
        self.lens_backend
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|e| e.google_lens_is_blocked())
    }

    fn google_lens_is_resting(&self) -> bool {
        self.lens_backend
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|e| e.google_lens_is_resting())
    }

    fn google_lens_resting_until(&self) -> String {
        self.lens_backend
            .lock()
            .unwrap()
            .as_ref()
            .map(|e| e.google_lens_resting_until())
            .unwrap_or_default()
    }

    fn google_lens_wait_for_turn<'a>(&'a self, on_tick: Option<OnTick>) -> BoxFuture<'a, ()> {
        let backend = self.lens_backend.lock().unwrap().clone();
        match backend {
            Some(engines) => {
                Box::pin(async move { engines.google_lens_wait_for_turn(on_tick).await })
            }
            None => panic!("Google Lens is not scripted"),
        }
    }

    fn google_lens<'a>(
        &'a self,
        path: &'a Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        whole_image: bool,
    ) -> BoxFuture<'a, Result<Vec<GoogleLensMatch>, LensError>> {
        let backend = self.lens_backend.lock().unwrap().clone();
        match backend {
            Some(engines) => Box::pin(async move {
                engines
                    .google_lens(path, timeout, on_tick, whole_image)
                    .await
            }),
            None => panic!("Google Lens is not scripted"),
        }
    }

    fn yandex_is_blocked(&self) -> bool {
        false
    }

    fn yandex_is_resting(&self) -> bool {
        false
    }

    fn yandex_resting_until(&self) -> String {
        String::new()
    }

    fn yandex_wait_for_turn<'a>(
        &'a self,
        _on_tick: Option<OnTick>,
        _should_stop: Option<ShouldStop>,
    ) -> BoxFuture<'a, ()> {
        panic!("Yandex is not scripted")
    }

    fn yandex<'a>(
        &'a self,
        _path: &'a Path,
        _timeout: f64,
        _on_tick: Option<OnTick>,
    ) -> BoxFuture<'a, Result<Vec<YandexMatch>, YandexError>> {
        panic!("Yandex is not scripted")
    }

    fn file_sha256<'a>(&'a self, _path: &'a Path) -> BoxFuture<'a, Result<String, String>> {
        panic!("Pawchive is not scripted")
    }

    fn pawchive_lookup<'a>(
        &'a self,
        _sha256: &'a str,
        _timeout: f64,
    ) -> BoxFuture<'a, Result<Vec<PawchiveMatch>, PawchiveError>> {
        panic!("Pawchive is not scripted")
    }

    fn pawchive_index<'a>(
        &'a self,
        path: &'a Path,
        hydrus_hash: Option<&'a str>,
    ) -> BoxFuture<'a, Result<IndexAnswer, IndexError>> {
        let backend = self.index_backend.lock().unwrap().clone();
        match backend {
            Some(engines) => {
                Box::pin(async move { engines.pawchive_index(path, hydrus_hash).await })
            }
            None => Box::pin(async { Ok(IndexAnswer::Empty) }),
        }
    }
}
