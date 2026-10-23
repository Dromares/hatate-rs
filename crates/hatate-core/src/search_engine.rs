//! Orchestrating one image's search (`core/search_engine.py`, Phase 3.h, DAN-2064): ask the
//! engines in waves, merge what they find, check each match is still there, fetch the best one's
//! page, tags and preview, and decide GOOD or POOR. The rest of the matches stay listed so the
//! user can flip through them.
//!
//! Everything this calls is an earlier ticket's: the engine runner (3.f), image compare and
//! multipage page picking (3.g), availability (3.d), the booru fetch and the net layer (Phase 1
//! and 2), the search cache (Phase 1). What is new here is the order they are called in, the
//! state `search_image` carries between them, and the glue between the in-memory [`Entry`] and the
//! cache file ([`apply_cached_result`], [`save_cached_result`]).
//!
//! **Settings.** Python reads one live `Settings` for the whole search. Here the caller owns it
//! behind a [`Mutex`] (a DeviantArt fetch can renew a cookie and save it, which writes it back),
//! and each step reads a copy taken when it starts. A change made by someone else in the middle of
//! a search is therefore seen by the next step that starts, not the one running; Python's sees it
//! at its next read, which is the same thing at a finer grain.
//!
//! **The candidate list.** Python holds `candidates` and `entry.candidates` as one list for most of
//! the function. Here `entry.candidates` is that list from the moment it is assigned, and the
//! places Python uses `list.index` or `in` on a candidate (which compare by value) find it by
//! value too.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use tracing::{debug, info, warn};

use crate::availability::{
    SettingsAccess, check_url_available, drop_structurally_dead, reactor_full_fallback,
    recheck_cached_candidates, redirect_means_gone, should_availability_check,
    soft_404_markers_for, sweep_candidate_availability,
};
use crate::boorus::BooruError;
use crate::boorus::fetch::{BooruSession, FetchOptions, SettingsSink, fetch_page_info};
use crate::cache::{self, CacheEntry, CacheKey, Lookup, SearchCache};
use crate::clock::Clock;
use crate::config::Settings;
use crate::engine_runner::{
    EngineRunner, OnEngineFinished, engine_waves, fallback_is_unnecessary, no_match,
};
use crate::engines::client::OnTick as EngineOnTick;
use crate::engines::saucenao::ShouldStop;
use crate::hydrus_tag_lookup::hash_file;
use crate::model::{Candidate, CandidateTag, Entry, MatchStatus, Tag};
use crate::multipage_resolve::{ResolveError, resolve_multipage_candidate};
use crate::net::NetClient;
use crate::net::site;
use crate::paths::Paths;
use crate::progress::OnTick;
use crate::pyimage::DecompressionBomb;
use crate::pyraster;
use crate::ranking::rank_candidates;
use crate::remote::{self, Raised as RemoteRaised, referer_for_candidate};
use crate::similarity_check::{
    SimilarityError, measure_ordinal_similarities, recheck_borrowed_measurement,
};
use crate::sites::classify_site;
use crate::tag_band_metrics::{self, EntryMeasurement, Recorder, bucket_for};
use crate::tag_borrowing::borrow_tags;
use crate::tag_rules::{
    apply_namespace_remap, apply_outcome_tags, apply_tag_blacklist, countable_tags, with_rating_tag,
};

/// `MAX_DEAD_CANDIDATE_RETRIES`: how many candidates to try before giving up on finding a live
/// one. Each attempt costs a booru page fetch.
pub const MAX_DEAD_CANDIDATE_RETRIES: usize = 4;

/// What leaves `search_image` as an exception.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchError {
    /// An engine's exception that is not its own error type, out of a wave of one engine.
    Engine(crate::engine_runner::Raised),
    /// From `remote.download_bytes`, `fetch_remote_info` or `check_url_available`.
    Remote(RemoteRaised),
    /// From `_resolve_multipage_candidate`.
    Resolve(ResolveError),
    /// From `measure_ordinal_similarities` or `_recheck_borrowed_measurement`.
    Similarity(SimilarityError),
    /// `Image.DecompressionBombError`, from reading the local file's size.
    Bomb(DecompressionBomb),
}

impl std::fmt::Display for SearchError {
    /// `str(exc)`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchError::Engine(e) => write!(f, "{e}"),
            SearchError::Remote(e) => write!(f, "{e}"),
            SearchError::Resolve(e) => write!(f, "{e}"),
            SearchError::Similarity(e) => write!(f, "{e}"),
            SearchError::Bomb(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<crate::engine_runner::Raised> for SearchError {
    fn from(e: crate::engine_runner::Raised) -> Self {
        SearchError::Engine(e)
    }
}
impl From<RemoteRaised> for SearchError {
    fn from(e: RemoteRaised) -> Self {
        SearchError::Remote(e)
    }
}
impl From<ResolveError> for SearchError {
    fn from(e: ResolveError) -> Self {
        SearchError::Resolve(e)
    }
}
impl From<SimilarityError> for SearchError {
    fn from(e: SimilarityError) -> Self {
        SearchError::Similarity(e)
    }
}
impl From<DecompressionBomb> for SearchError {
    fn from(e: DecompressionBomb) -> Self {
        SearchError::Bomb(e)
    }
}

impl SearchError {
    /// `type(exc).__name__`, where this crate knows it. `None` for an engine's own unexpected
    /// exception (`Raised(String)` carries the message only; `docs/search-engine.md`).
    pub fn class_name(&self) -> Option<&'static str> {
        fn remote(e: &RemoteRaised) -> &'static str {
            match e {
                RemoteRaised::HardTimeout(_) => "HardTimeoutError",
                RemoteRaised::ValueError(_) => "ValueError",
                RemoteRaised::UnicodeEncode(_) => "UnicodeEncodeError",
                RemoteRaised::BodyRead { class, .. } => class,
            }
        }
        match self {
            SearchError::Engine(_) => None,
            SearchError::Remote(e) => Some(remote(e)),
            SearchError::Resolve(ResolveError::Raised(e)) => Some(remote(e)),
            SearchError::Resolve(ResolveError::Bomb(_)) => Some("DecompressionBombError"),
            SearchError::Resolve(ResolveError::Url(_)) => Some("ValueError"),
            SearchError::Similarity(SimilarityError::Raised(e)) => Some(remote(e)),
            SearchError::Similarity(SimilarityError::Bomb(_)) => Some("DecompressionBombError"),
            SearchError::Bomb(_) => Some("DecompressionBombError"),
        }
    }
}

/// `search_image`'s optional arguments.
pub struct SearchOptions<'a> {
    /// Force one engine (`override_engine`).
    pub override_engine: Option<&'a str>,
    /// Look the file up in the search cache first (`use_cache`, default true).
    pub use_cache: bool,
    /// `on_tick(label, remaining, total)`: about once a second while a request is in flight.
    pub on_tick: Option<EngineOnTick>,
    /// Fires once, the moment the last engine request completes and before any booru page is
    /// fetched. Not at all when the result came from the cache.
    pub on_engines_done: Option<&'a (dyn Fn() + Send + Sync)>,
    /// Fires as each engine completes, so the caller can pace each host separately.
    pub on_engine_finished: Option<OnEngineFinished<'a>>,
    pub should_stop: Option<ShouldStop>,
    pub skip_saucenao: bool,
}

impl Default for SearchOptions<'_> {
    fn default() -> Self {
        SearchOptions {
            override_engine: None,
            use_cache: true,
            on_tick: None,
            on_engines_done: None,
            on_engine_finished: None,
            should_stop: None,
            skip_saucenao: false,
        }
    }
}

/// The pieces a search needs, owned once per process: the net client, the booru fetch state
/// (parser health, the Data-API latch, the file-hash cache), the engine runner, the cache and
/// the profile paths.
pub struct SearchEngine {
    pub net: NetClient,
    pub booru: BooruSession,
    pub runner: EngineRunner,
    pub cache: SearchCache,
    pub paths: Paths,
    pub clock: Arc<dyn Clock>,
    /// The tag-band recorder (`tag_band_recorder`), resolved once when this is built: reading the
    /// variable per search would let a mid-run change take effect halfway through a measurement.
    tag_band: Option<Recorder>,
}

impl SearchEngine {
    /// A search engine whose tag-band recorder is resolved from the process environment
    /// (`HATATE_TAG_BAND_METRICS`).
    pub fn new(
        net: NetClient,
        runner: EngineRunner,
        paths: Paths,
        clock: Arc<dyn Clock>,
    ) -> SearchEngine {
        Self::with_recorder(
            net,
            runner,
            paths,
            clock,
            Recorder::from_env(|k| std::env::var(k).ok()),
        )
    }

    /// As [`SearchEngine::new`] with the recorder given (or none), for a caller that has already
    /// resolved it, and for tests.
    pub fn with_recorder(
        net: NetClient,
        runner: EngineRunner,
        paths: Paths,
        clock: Arc<dyn Clock>,
        tag_band: Option<Recorder>,
    ) -> SearchEngine {
        let cache = SearchCache::new(&paths);
        SearchEngine {
            net,
            booru: BooruSession::new(),
            runner,
            cache,
            paths,
            clock,
            tag_band,
        }
    }

    /// `tag_band_recorder`: the process's measurement recorder, or `None` when not measuring.
    pub fn tag_band_recorder(&self) -> Option<&Recorder> {
        self.tag_band.as_ref()
    }

    fn access<'a>(&'a self, settings: &'a Mutex<Settings>) -> SettingsAccess<'a> {
        SettingsAccess {
            settings,
            paths: &self.paths,
        }
    }

    /// `_measure_tag_band`: snapshots the tag/lender picture BEFORE the tagless rescue runs, so a
    /// rescued entry measured afterwards does not make the tagless band read as empty.
    fn measure_tag_band(
        &self,
        entry: &Entry,
        chosen_index: usize,
        settings: &Settings,
    ) -> Option<EntryMeasurement> {
        self.tag_band.as_ref()?;
        Some(tag_band_metrics::measure_entry(
            entry.tags.len() as i64,
            &entry.candidates,
            chosen_index,
            settings.match_conditions.min_tags_for_good,
            settings.borrow_tags_similarity_slack,
            true,
        ))
    }

    /// `_finish_tag_band_measurement`: completes the snapshot with the final tag count and writes
    /// the row. The count is `countable_tags`, not `entry.tags`: the by-rule tags are the app's
    /// own bookkeeping, and counting them would inflate the band.
    fn finish_tag_band_measurement(
        &self,
        measurement: Option<EntryMeasurement>,
        entry: &Entry,
        settings: &Settings,
        all_matches_dead: bool,
    ) {
        let (Some(recorder), Some(mut m)) = (self.tag_band.as_ref(), measurement) else {
            return;
        };
        m.all_matches_dead = all_matches_dead;
        m.entry_tag_count = countable_tags(&entry.tags).len() as i64;
        m.entry_bucket = bucket_for(
            m.entry_tag_count,
            settings.match_conditions.min_tags_for_good,
        )
        .to_owned();
        recorder.record(&m);
    }

    /// `fetch_candidate_details`: fills in a candidate's thumbnail bytes, booru-page tags, direct
    /// file URL, preview URL, and remote format/size info, mutating it in place. Safe to call more
    /// than once: it skips work already done. Used for the top pick right after searching, and
    /// lazily when the user picks a different candidate.
    pub async fn fetch_candidate_details(
        &self,
        candidate: &mut Candidate,
        settings: &Mutex<Settings>,
        on_tick: Option<OnTick<'_>>,
        local_path: Option<&str>,
    ) -> Result<(), SearchError> {
        let local_path = local_path.filter(|p| !p.is_empty());
        if !candidate.booru_tags_fetched {
            candidate.booru_tags_fetched = true;
            let mut snap = settings.lock().unwrap().clone();
            if snap.retrieve_tags_from_booru && !candidate.url.is_empty() {
                self.fetch_booru_details(candidate, settings, &mut snap, on_tick, local_path)
                    .await?;
            }
        }

        let snap = settings.lock().unwrap().clone();
        if candidate.remote_available.is_none()
            && snap.drop_dead_matches
            && !candidate.url.is_empty()
            && should_availability_check(&candidate.url)
        {
            // Sites with no booru parser (DeviantArt, etc.) never went through the fetch above,
            // so nothing has established whether they're still alive. A deleted deviation is the
            // dead link worth dropping: a single cheap request that reads only the status line
            // and headers.
            let creds = remote::credentials(&snap);
            let url = candidate.url.clone();
            let verdict = check_url_available(
                &self.net,
                Some(&url),
                snap.search_timeout,
                None,
                on_tick,
                site::cookies_for_url(&url, &creds),
                Some(&self.access(settings)),
            )
            .await?;
            let verdict = reactor_full_fallback(candidate, verdict);
            candidate.record_availability(verdict, self.clock.wall());
        }

        if candidate.thumb_bytes.is_none() {
            // Prefer the booru's own mid-resolution sample image over the search engine's tiny
            // thumbnail when there is one; fall back to the thumbnail.
            let preview_source = candidate
                .preview_url
                .clone()
                .filter(|u| !u.is_empty())
                .or_else(|| candidate.thumb_url.clone().filter(|u| !u.is_empty()));
            if let Some(source) = preview_source {
                // The same hotlink-protection concern as the format/size check below.
                let referer = referer_for_candidate(candidate, Some(&source));
                let creds = remote::credentials(&snap);
                candidate.thumb_bytes = remote::download_bytes(
                    &self.net,
                    Some(&source),
                    snap.search_timeout,
                    referer.as_deref(),
                    on_tick,
                    site::cookies_for_url(&source, &creds),
                )
                .await?;
            }
        }

        if !candidate.remote_info_fetched {
            candidate.remote_info_fetched = true;
            let has_format = candidate
                .remote_format
                .as_deref()
                .is_some_and(|f| !f.is_empty());
            let has_size = candidate.remote_size_bytes.is_some_and(|s| s != 0);
            if has_format && has_size {
                // The site already stated both, about the real original. A HEAD could only agree
                // at best, and on a site whose original isn't reachable it would fall back to the
                // search engine's thumbnail and report THAT as the match's format and size.
                debug!(
                    "{}: format/size came from the site itself ({}, {} bytes) - skipping the HEAD",
                    candidate.url,
                    candidate.remote_format.as_deref().unwrap_or_default(),
                    candidate.remote_size_bytes.unwrap_or_default()
                );
            } else {
                // The actual image file URL when we have one (from the booru page): HEADing the
                // post page itself just reports "text/html". Fall back to the thumbnail URL
                // rather than ever HEADing the HTML page.
                let info_url = candidate
                    .direct_file_url
                    .clone()
                    .filter(|u| !u.is_empty())
                    .or_else(|| candidate.thumb_url.clone().filter(|u| !u.is_empty()));
                // Several image CDNs enforce hotlink protection and serve an HTML block page to a
                // request with no (or the wrong) Referer.
                let referer = referer_for_candidate(candidate, info_url.as_deref());
                let (fmt, size_bytes) = remote::fetch_remote_info(
                    &self.net,
                    info_url.as_deref(),
                    snap.search_timeout,
                    referer.as_deref(),
                    on_tick,
                )
                .await?;
                // Never let the HEAD blank out a value the site stated itself: it may have
                // supplied only one of the two.
                if !has_format {
                    candidate.remote_format = fmt;
                }
                if !has_size {
                    candidate.remote_size_bytes = size_bytes;
                }
            }
        }
        Ok(())
    }

    /// The `if settings.retrieve_tags_from_booru and candidate.url:` body of
    /// `fetch_candidate_details`: the booru page fetch and what is read from it.
    async fn fetch_booru_details(
        &self,
        candidate: &mut Candidate,
        shared: &Mutex<Settings>,
        snap: &mut Settings,
        on_tick: Option<OnTick<'_>>,
        local_path: Option<&str>,
    ) -> Result<(), SearchError> {
        let url = candidate.url.clone();
        let creds = remote::credentials(snap);
        let before_cookies = snap.deviantart_cookies.clone();
        let gone_redirect = |u: &str, f: Option<&str>| redirect_means_gone(u, f).map(str::to_owned);
        let fetched = fetch_page_info(
            &self.net,
            &self.booru,
            &url,
            FetchOptions {
                timeout: snap.search_timeout,
                on_tick,
                cookies: site::cookies_for_url(&url, &creds),
                headers: site::headers_for_url(&url, Some(&creds)),
                gone_markers: soft_404_markers_for(&url),
                local_path: local_path.map(Path::new),
                gone_redirect: Some(&gone_redirect),
                // The snapshot takes the renewed cookie and saves it; the shared settings are
                // given it below so the next step reads it.
                settings: Some(SettingsSink {
                    settings: &mut *snap,
                    paths: &self.paths,
                }),
            },
        )
        .await;
        if snap.deviantart_cookies != before_cookies {
            shared.lock().unwrap().deviantart_cookies = snap.deviantart_cookies.clone();
        }

        let page_info = match fetched {
            Ok(page_info) => page_info,
            Err(BooruError::ContentGone(_)) => {
                // The post is definitively deleted/removed: recorded here so it doesn't have to
                // be rediscovered later by a separate availability check. Only a definite 404/410
                // gets here; a plain `BooruError` could be a timeout or a rate limit.
                candidate.record_availability(Some(false), self.clock.wall());
                candidate.booru_tags = Vec::new();
                return Ok(());
            }
            Err(BooruError::Fetch(message)) => {
                // A timeout, connection failure, or non-200 response says nothing about whether
                // the post still exists, so `remote_available` is left alone. Resetting
                // `booru_tags_fetched` lets a later selection of the same candidate retry.
                warn!(
                    "Could not fetch booru tags for {}: {message}",
                    candidate.url
                );
                candidate.booru_tags = Vec::new();
                candidate.incomplete_reason = Some(message);
                candidate.booru_tags_fetched = false;
                return Ok(());
            }
        };

        if page_info
            .gone_reason
            .as_deref()
            .is_some_and(|r| !r.is_empty())
        {
            // The page itself says the post is deleted. This is the only check a single-candidate
            // match gets: the availability sweep only runs when there is more than one candidate.
            candidate.record_availability(Some(false), self.clock.wall());
        }
        let tags = apply_tag_blacklist(
            &apply_namespace_remap(
                &with_rating_tag(&page_info.tags, page_info.rating.as_deref(), snap),
                snap,
            ),
            snap,
        );
        candidate.booru_tags = candidate_tags(&tags);
        candidate.direct_file_url = page_info.file_url.clone();
        candidate.preview_url = page_info.preview_url.clone();
        if let Some(canonical) = page_info.canonical_url.as_deref() {
            if !canonical.is_empty() && canonical != candidate.url {
                // The URL we were given is a dead legacy route; now that the post has been
                // identified, point at where it actually lives.
                info!(
                    "Repointing {} to its current address {canonical}",
                    candidate.url
                );
                candidate.url = canonical.to_owned();
            }
        }
        // IQDB already gives us dimensions for its own matches; for SauceNAO-origin candidates we
        // don't have them until now.
        if candidate.width.is_none() && page_info.width.is_some_and(|w| w != 0) {
            candidate.width = page_info.width;
        }
        if candidate.height.is_none() && page_info.height.is_some_and(|h| h != 0) {
            candidate.height = page_info.height;
        }
        // A site that states these describes its ORIGINAL, which a HEAD can only do when the
        // original is actually reachable.
        if page_info
            .file_format
            .as_deref()
            .is_some_and(|f| !f.is_empty())
            && candidate.remote_format.as_deref().is_none_or(str::is_empty)
        {
            candidate.remote_format = page_info.file_format.clone();
        }
        if page_info.file_size_bytes.is_some_and(|s| s != 0)
            && candidate.remote_size_bytes.is_none_or(|s| s == 0)
        {
            candidate.remote_size_bytes = page_info.file_size_bytes;
        }
        if page_info.page_count > 1 && local_path.is_some() {
            resolve_multipage_candidate(
                &self.net,
                candidate,
                page_info.page_count,
                snap,
                local_path.unwrap_or_default(),
                Some(&page_info),
            )
            .await?;
        } else if let Some(local) = local_path {
            recheck_borrowed_measurement(&self.net, candidate, &page_info, snap, local).await?;
        }
        candidate.incomplete_reason = page_info.incomplete_reason.clone();
        candidate.restricted = page_info.restricted.clone();
        let restricted = page_info
            .restricted
            .as_deref()
            .is_some_and(|r| !r.is_empty());
        if restricted && snap.drop_restricted_matches {
            // The post exists, but the site won't serve its image without a paid account tier.
            // Marked unavailable so the dead-match handling drops it: from this app's side the
            // outcome is the same. Kept as a SEPARATE flag from the drop so the reason survives.
            info!(
                "Dropping {} - {}",
                candidate.url,
                page_info.restricted.as_deref().unwrap_or_default()
            );
            candidate.record_availability(Some(false), self.clock.wall());
        } else if page_info.fetched && candidate.remote_available.is_none() {
            // Only meaningful when a parser actually fetched the page (a site with no parser
            // returns an empty result WITHOUT raising), and only when nothing has already
            // decided: a page that SAYS the post is deleted was still fetched.
            candidate.record_availability(Some(true), self.clock.wall());
        }
        Ok(())
    }

    /// `search_image`: searches one image and fills in `entry`. The caller's callbacks are the
    /// Python ones: `on_tick` for UI countdown feedback, `on_engines_done` once the last engine
    /// request completes, `on_engine_finished` as each engine does, `should_stop` for a stop flag
    /// checked while a wait runs.
    pub async fn search_image(
        &self,
        entry: &mut Entry,
        settings: &Mutex<Settings>,
        opts: SearchOptions<'_>,
    ) -> Result<(), SearchError> {
        let filename = entry.filename().to_owned();
        info!("Searching {filename}");
        entry.status = MatchStatus::Searching;
        entry.error_message = None;
        entry.candidates = Vec::new();

        let s = Arc::new(settings.lock().unwrap().clone());
        let path = entry.path.clone();

        load_local_dimensions(entry)?;

        if entry.hydrus_hash.as_deref().is_none_or(str::is_empty) {
            entry.hydrus_hash = Some(hash_file(Path::new(&entry.path)));
        }

        // Serialises the caller's callback across the engines of a wave, which run at once.
        let tick: Option<EngineOnTick> = opts.on_tick.clone().map(|inner| {
            let lock = Arc::new(Mutex::new(()));
            Arc::new(move |label: &str, remaining: f64, total: f64| {
                let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
                inner(label, remaining, total);
            }) as EngineOnTick
        });

        let cache_key = entry
            .hydrus_hash
            .as_deref()
            .filter(|h| !h.is_empty())
            .and_then(|h| CacheKey::parse(h).ok());
        if opts.use_cache {
            if let Some(key) = cache_key.as_ref() {
                if let Lookup::Hit(cached) =
                    self.cache
                        .lookup(key, s.search_cache_ttl_days, self.clock.wall())
                {
                    apply_cached_result(entry, &cached);
                    // A result cached before a site's URLs stopped resolving still carries them.
                    // Cheap to clean up here, and the alternative (throwing the whole cache away)
                    // would mean re-searching every image at 45-75s each.
                    let before = entry.candidates.len();
                    let mut cleaned =
                        drop_structurally_dead(std::mem::take(&mut entry.candidates), &filename);
                    if s.drop_dead_matches {
                        cleaned = recheck_cached_candidates(
                            &self.net,
                            cleaned,
                            &self.access(settings),
                            &filename,
                            tick.as_deref(),
                        )
                        .await;
                    }
                    let changed = cleaned.len() != before;
                    let none_left = cleaned.is_empty();
                    entry.candidates = cleaned;
                    if changed {
                        // `select_candidate(0 if cleaned else -1)`: -1 changes nothing.
                        if !none_left {
                            entry.select_candidate(0);
                        }
                    }
                    info!(
                        "{filename}: using cached search result ({} candidate(s), status={}) - \
                         skipping IQDB/SauceNAO",
                        entry.candidates.len(),
                        entry.status.as_str()
                    );
                    if !entry.candidates.is_empty() {
                        let index = entry.selected_candidate_index;
                        self.fetch_candidate_details(
                            &mut entry.candidates[index],
                            settings,
                            tick.as_deref(),
                            Some(&path),
                        )
                        .await?;
                        // Re-sync now that `thumb_bytes` is filled in.
                        entry.select_candidate(index);
                    }
                    entry.result_source = Some("cached".to_owned());
                    self.mark_searched(entry);
                    // The cache stores status and candidates but NOT `entry.tags`, so the by-rule
                    // tags are not restored with the result and have to be re-derived here.
                    apply_outcome_tags(entry, &s);
                    return Ok(());
                }
            }
        }

        // A genuinely fresh search produces a NEW result, so this entry is eligible to be sent
        // again. Re-sending is safe: Hydrus's file import is idempotent.
        entry.sent_to_hydrus = false;
        entry.hydrus_import_confirmed = false;

        let mut run = Run {
            s: s.clone(),
            tick: tick.clone(),
            on_engine_finished: opts.on_engine_finished,
            should_stop: opts.should_stop.clone(),
            errors: Vec::new(),
            answered: HashSet::new(),
            deferred_wave: None,
        };
        let mut candidates: Vec<Candidate> = Vec::new();

        let waves = engine_waves(&s, opts.override_engine, opts.skip_saucenao);
        let mut found_by_engine: HashMap<String, bool> = HashMap::new();
        for (i, wave) in waves.iter().enumerate() {
            if wave.is_empty() {
                continue;
            }
            // The last wave is a fallback: it runs only when what has been found so far is not
            // good enough. What "good enough" means is the user's call.
            if waves.len() > 1 && i == waves.len() - 1 {
                let (skip, why) =
                    fallback_is_unnecessary(&s, &waves, &found_by_engine, &candidates);
                if skip {
                    debug!("{filename}: skipping fallback engines {wave:?} - {why}");
                    run.deferred_wave = Some(wave.clone());
                    break;
                }
                debug!("{filename}: running fallback engines {wave:?} - {why}");
            }
            let wave_found = self
                .runner
                .run_engines_parallel(
                    wave,
                    entry,
                    &s,
                    &mut candidates,
                    &mut run.errors,
                    tick.clone(),
                    run.on_engine_finished,
                    run.should_stop.clone(),
                    Some(&mut run.answered),
                )
                .await?;
            found_by_engine.extend(wave_found);
        }

        if let Some(done) = opts.on_engines_done {
            // Every engine request for this image is now done. Everything below talks to booru
            // hosts, not to IQDB/SauceNAO.
            done();
        }

        let mut candidates = dedupe(candidates);
        let before_site_filter = candidates.len();
        candidates.retain(|c| {
            s.enabled_sites
                .contains(&classify_site(c.source_name.as_deref(), &c.url))
        });
        if candidates.len() != before_site_filter {
            debug!(
                "{filename}: {} candidate(s) dropped by Sites filter ({before_site_filter} -> {})",
                before_site_filter - candidates.len(),
                candidates.len()
            );
        }
        // Before the sort, so the order reflects the real numbers rather than the positions the
        // engines made up. After the site filter, so nothing is downloaded for a candidate that is
        // about to be dropped.
        measure_ordinal_similarities(&self.net, entry, &mut candidates, &s).await?;
        sort_by_similarity(&mut candidates);
        entry.candidates = candidates;
        info!(
            "{filename}: {} unique candidate(s) after merge and site filter",
            entry.candidates.len()
        );

        if entry.candidates.is_empty()
            && self
                .run_deferred_engines(&mut run, entry, "every match was filtered out")
                .await?
        {
            info!(
                "{filename}: the fallback engines found {} candidate(s)",
                entry.candidates.len()
            );
        }

        if entry.candidates.is_empty() {
            let (status, message, is_final) = no_match(&run.errors, &run.answered);
            entry.status = status;
            entry.error_message = message;
            entry.result_source = Some("fresh".to_owned());
            entry.searched_without_saucenao = opts.skip_saucenao;
            self.mark_searched(entry);
            // `no_match` can return ERROR as well as NOT_FOUND; `apply_outcome_tags` adds nothing
            // for ERROR, so this stays a no-op for a search that merely failed.
            apply_outcome_tags(entry, &s);
            if is_final {
                self.save_cached_result(entry);
            }
            return Ok(());
        }

        if s.drop_dead_matches && entry.candidates.len() > 1 {
            // Check EVERY candidate's availability up front, not just the ones we'd fetch details
            // for: otherwise a dead match further down the list survives into the dropdown and is
            // only discovered when the user clicks it. A cheap status-only request per candidate,
            // run concurrently since they hit different hosts.
            sweep_candidate_availability(
                &self.net,
                &mut entry.candidates,
                &self.access(settings),
                &filename,
                tick.as_deref(),
            )
            .await;
        }

        if s.rank_matches_by_quality {
            // Re-ordered only now, once availability is known: ranking before the sweep would
            // recommend a match that's about to be found dead.
            entry.candidates = rank_candidates(
                std::mem::take(&mut entry.candidates),
                entry.local_width,
                entry.local_height,
                &filename,
            );
        }

        // Fetch details for the best candidate, but if that fetch reveals the post is definitively
        // gone (a 404/410, not a transient failure), it's not a usable match, so try the next-best
        // one instead of settling on a dead link. Bounded so a run of dead matches can't turn one
        // search into a long chain of fetches.
        let max_attempts = entry.candidates.len().min(MAX_DEAD_CANDIDATE_RETRIES);
        let mut chosen_index = 0;
        let mut rescored = false;
        let mut chosen = false;
        for attempt in 0..max_attempts {
            if entry.candidates[attempt].remote_available == Some(false) {
                continue; // already known dead from the sweep - don't waste a detail fetch on it
            }
            let score_before = entry.candidates[attempt].similarity;
            self.fetch_candidate_details(
                &mut entry.candidates[attempt],
                settings,
                tick.as_deref(),
                Some(&path),
            )
            .await?;
            if entry.candidates[attempt].remote_available == Some(false) {
                info!(
                    "{filename}: candidate {} ({}) is gone, trying the next best match",
                    attempt + 1,
                    entry.candidates[attempt].url
                );
                continue;
            }
            if entry.candidates[attempt].similarity < score_before - 1.0 {
                // Its score came from a picture that wasn't its own (see
                // `recheck_borrowed_measurement`). Not the best match after all.
                rescored = true;
                info!(
                    "{filename}: candidate {} ({}) re-scored {score_before:.0}% -> {:.0}%, \
                     trying the next best",
                    attempt + 1,
                    entry.candidates[attempt].url,
                    entry.candidates[attempt].similarity
                );
                continue;
            }
            chosen_index = attempt;
            chosen = true;
            break;
        }
        if !chosen {
            // Every candidate we checked was gone: keep the first one selected so the entry still
            // shows something, but the status logic below and the dead-link filter will reflect
            // reality.
            chosen_index = 0;
        }

        if rescored {
            // Put the re-scored ones where their real numbers belong, keeping the choice made
            // above.
            let chosen_candidate = entry.candidates[chosen_index].clone();
            entry.candidates = rank_candidates(
                std::mem::take(&mut entry.candidates),
                entry.local_width,
                entry.local_height,
                &filename,
            );
            chosen_index = entry
                .candidates
                .iter()
                .position(|c| *c == chosen_candidate)
                .unwrap_or(0);
        }

        // Read-only, and deliberately ahead of the borrow below. A no-op unless the run was
        // started with the measurement environment variable set.
        let tag_band = self.measure_tag_band(entry, chosen_index, &s);

        // A chosen match with no tags is not always a dead end: another match of the SAME picture
        // may carry them. See `tag_borrowing`.
        if entry
            .candidates
            .get(chosen_index)
            .is_some_and(|c| c.booru_tags.is_empty())
        {
            borrow_tags(
                &filename,
                &mut entry.candidates,
                chosen_index,
                &s,
                async |c: &mut Candidate| {
                    self.fetch_candidate_details(c, settings, tick.as_deref(), Some(&path))
                        .await
                },
            )
            .await?;
        }

        if s.drop_dead_matches {
            let live = entry
                .candidates
                .iter()
                .filter(|c| c.remote_available != Some(false))
                .count();
            let dropped = entry.candidates.len() - live;
            if dropped > 0 {
                info!("{filename}: dropped {dropped} match(es) confirmed gone");
            }
            if live > 0 {
                let chosen_candidate = entry.candidates[chosen_index].clone();
                entry
                    .candidates
                    .retain(|c| c.remote_available != Some(false));
                chosen_index = entry
                    .candidates
                    .iter()
                    .position(|c| *c == chosen_candidate)
                    .unwrap_or(0);
            } else if dropped > 0
                && self
                    .run_deferred_engines(&mut run, entry, "every match was dead")
                    .await?
            {
                // The fallback engines had been skipped for a match that has since turned out to
                // be a broken link. They get their turn.
                chosen_index = 0;
            } else if dropped > 0 {
                // Every match was dead: report honestly rather than presenting a broken link as a
                // good result.
                entry.candidates = Vec::new();
                entry.status = MatchStatus::NotFound;
                entry.result_source = Some("fresh".to_owned());
                entry.searched_without_saucenao = opts.skip_saucenao;
                self.mark_searched(entry);
                apply_outcome_tags(entry, &s);
                self.finish_tag_band_measurement(tag_band, entry, &s, true);
                self.save_cached_result(entry);
                return Ok(());
            }
        }

        entry.select_candidate(chosen_index);

        if !run.errors.is_empty() {
            // Partial failure, but we still got results.
            entry.error_message = Some(run.errors.join("; "));
        }

        entry.status = decide_status(entry, &s);
        entry.result_source = Some("fresh".to_owned());
        entry.searched_without_saucenao = opts.skip_saucenao;
        self.mark_searched(entry);
        apply_outcome_tags(entry, &s);
        // Now that `entry.tags` is final, the snapshot taken above can be completed and written.
        // This is the number `decide_status` just used.
        self.finish_tag_band_measurement(tag_band, entry, &s, false);
        self.save_cached_result(entry);
        Ok(())
    }

    /// `run_deferred_engines`: runs the fallback engines after all, and reports whether they
    /// turned anything up.
    ///
    /// They were skipped because something good enough had been found. When that something is
    /// thrown away afterwards (filtered out by site, or found to be dead) the reason for skipping
    /// them is gone with it, and the image would otherwise be recorded as NOT_FOUND while engines
    /// that were never asked sat in the plan.
    async fn run_deferred_engines(
        &self,
        run: &mut Run<'_>,
        entry: &mut Entry,
        reason: &str,
    ) -> Result<bool, SearchError> {
        let Some(wave) = run.deferred_wave.take() else {
            return Ok(false); // one retry, not a loop
        };
        info!(
            "{}: {reason} - running the fallback engines {wave:?} after all",
            entry.filename()
        );
        let mut candidates = std::mem::take(&mut entry.candidates);
        let ran = self
            .runner
            .run_engines_parallel(
                &wave,
                entry,
                &run.s,
                &mut candidates,
                &mut run.errors,
                run.tick.clone(),
                run.on_engine_finished,
                run.should_stop.clone(),
                Some(&mut run.answered),
            )
            .await;
        if let Err(raised) = ran {
            entry.candidates = candidates;
            return Err(raised.into());
        }
        let mut candidates = dedupe(candidates);
        // Anything already proved gone stays gone. One of the two callers is the "every match was
        // dead" branch, which computes the live list but does not assign it, so the dead ones are
        // still here, and rebuilding the list without this would put the very match that
        // triggered the retry straight back into the dropdown.
        candidates.retain(|c| {
            c.remote_available != Some(false)
                && run
                    .s
                    .enabled_sites
                    .contains(&classify_site(c.source_name.as_deref(), &c.url))
        });
        let measured =
            measure_ordinal_similarities(&self.net, entry, &mut candidates, &run.s).await;
        if let Err(e) = measured {
            entry.candidates = candidates;
            return Err(e.into());
        }
        sort_by_similarity(&mut candidates);
        entry.candidates = candidates;
        Ok(!entry.candidates.is_empty())
    }

    /// `ImageEntry.mark_searched`.
    fn mark_searched(&self, entry: &mut Entry) {
        entry.last_searched = Some(self.clock.wall());
    }

    /// `save_cached_result`, for an entry with a usable hash: nothing for a hash that is not 64
    /// lowercase hex digits (see [`CacheKey`]), and a file that cannot be written is a warning.
    fn save_cached_result(&self, entry: &Entry) {
        let Some(key) = entry
            .hydrus_hash
            .as_deref()
            .filter(|h| !h.is_empty())
            .and_then(|h| CacheKey::parse(h).ok())
        else {
            return;
        };
        let cached = cache_entry_for(entry, self.clock.wall());
        match self
            .cache
            .store(&key, &cached, entry.searched_without_saucenao)
        {
            Ok(cache::StoreOutcome::Written) => debug!(
                "Saved search cache entry for {} ({} candidate(s))",
                &key.as_str()[..12],
                entry.candidates.len()
            ),
            Ok(cache::StoreOutcome::SkippedProvisional) => debug!(
                "Not caching {} - searched without SauceNAO, so the result is provisional",
                entry.filename()
            ),
            Ok(cache::StoreOutcome::SkippedError) => {}
            Err(e) => warn!("Could not write search cache entry: {e}"),
        }
    }
}

/// What `search_image` carries from one wave to the next and into the deferred run.
struct Run<'a> {
    s: Arc<Settings>,
    tick: Option<EngineOnTick>,
    on_engine_finished: Option<OnEngineFinished<'a>>,
    should_stop: Option<ShouldStop>,
    /// One `"<engine>: <message>"` per engine that failed.
    errors: Vec<String>,
    /// Engines that searched and came back without an error: a real "here is what I found", even
    /// when that is nothing. See `no_match`.
    answered: HashSet<String>,
    /// The fallback engines, if they were skipped. Kept because the result they were skipped FOR
    /// can still be thrown away afterwards, and an image should not end up with nothing while
    /// engines that were never asked sit in the plan.
    deferred_wave: Option<Vec<&'static str>>,
}

/// `Tag`s as the `booru_tags` a candidate holds (the list implies the source).
fn candidate_tags(tags: &[Tag]) -> Vec<CandidateTag> {
    tags.iter()
        .map(|t| CandidateTag {
            name: t.name.clone(),
            namespace: t.namespace.clone(),
        })
        .collect()
}

/// `candidates.sort(key=lambda c: -c.similarity)`: stable, best first.
fn sort_by_similarity(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

/// `_dedupe`: the same picture can come back from both engines (or from IQDB checking more than
/// one mirror); keep the highest-similarity copy per URL. A URL keeps the place of its first
/// appearance, as a dict does when a later value replaces an earlier one.
pub fn dedupe(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut best: Vec<Candidate> = Vec::new();
    let mut index_by_url: HashMap<String, usize> = HashMap::new();
    for c in candidates {
        match index_by_url.get(&c.url) {
            Some(&i) => {
                if c.similarity > best[i].similarity {
                    best[i] = c;
                }
            }
            None => {
                index_by_url.insert(c.url.clone(), best.len());
                best.push(c);
            }
        }
    }
    best
}

/// `_decide_status`: GOOD (green) vs POOR (yellow). Few retrieved tags, or the matched image
/// looking better/larger than the local one, both suggest the user should review it manually.
///
/// The tag count is `countable_tags`, not `len(entry.tags)`: a re-search keeps the by-rule tags a
/// previous search left behind, so counting them would let `hatate:few tags` count towards the
/// threshold it is a report of and eventually promote the entry to GOOD.
pub fn decide_status(entry: &Entry, settings: &Settings) -> MatchStatus {
    let cond = &settings.match_conditions;
    let tag_count = countable_tags(&entry.tags).len() as i64;

    if tag_count < cond.min_tags_for_good {
        return MatchStatus::Poor;
    }

    // `entry.match_width` and `match_height` are the selected candidate's.
    let (match_width, match_height) = entry
        .selected_candidate()
        .map_or((None, None), |c| (c.width, c.height));
    if cond.require_larger_than_local {
        if let (Some(local_width), Some(match_width)) = (
            entry.local_width.filter(|&w| w != 0),
            match_width.filter(|&w| w != 0),
        ) {
            let width_gain = (match_width - local_width) as f64 / local_width.max(1) as f64 * 100.0;
            let mut height_gain = 0.0;
            if let (Some(local_height), Some(match_height)) = (
                entry.local_height.filter(|&h| h != 0),
                match_height.filter(|&h| h != 0),
            ) {
                height_gain =
                    (match_height - local_height) as f64 / local_height.max(1) as f64 * 100.0;
            }
            if width_gain >= cond.min_width_gain_percent
                || height_gain >= cond.min_height_gain_percent
            {
                return MatchStatus::Poor;
            }
        }
    }

    MatchStatus::Good
}

/// `_load_local_dimensions`: the local file's size, from its header. A file that cannot be read
/// or is not an image leaves the entry as it was; a picture past Pillow's decompression-bomb
/// limit raises.
fn load_local_dimensions(entry: &mut Entry) -> Result<(), DecompressionBomb> {
    let Ok(data) = std::fs::read(&entry.path) else {
        return Ok(());
    };
    if let Some((w, h)) = pyraster::pillow_size(&data)? {
        entry.local_width = Some(i64::from(w));
        entry.local_height = Some(i64::from(h));
    }
    Ok(())
}

// ----------------------------------------------------------------------
// The search cache, between the entry and the file
// ----------------------------------------------------------------------

fn from_cache_status(status: cache::MatchStatus) -> MatchStatus {
    MatchStatus::parse(status.as_str()).unwrap_or(MatchStatus::NotFound)
}

fn to_cache_status(status: MatchStatus) -> cache::MatchStatus {
    cache::MatchStatus::parse(status.as_str()).unwrap_or(cache::MatchStatus::NotFound)
}

fn from_cache_tags(tags: &[cache::Tag]) -> Vec<CandidateTag> {
    tags.iter()
        .map(|t| CandidateTag {
            name: t.name.clone(),
            namespace: t.namespace.clone(),
        })
        .collect()
}

fn to_cache_tags(tags: &[CandidateTag]) -> Vec<cache::Tag> {
    tags.iter()
        .map(|t| cache::Tag {
            name: t.name.clone(),
            namespace: t.namespace.clone(),
        })
        .collect()
}

/// `_candidate_from_dict`, from the cache's reading of the same dict.
pub fn candidate_from_cache(c: &cache::Candidate) -> Candidate {
    let mut out = Candidate::new(c.url.clone());
    out.source_name = c.source_name.clone();
    out.thumb_url = c.thumb_url.clone();
    out.similarity = c.similarity;
    out.similarity_measured = c.similarity_measured;
    out.width = c.width;
    out.height = c.height;
    out.engine = c.engine.clone();
    out.engine_tags = from_cache_tags(&c.engine_tags);
    out.booru_tags = from_cache_tags(&c.booru_tags);
    out.direct_file_url = c.direct_file_url.clone();
    out.preview_url = c.preview_url.clone();
    out.remote_format = c.remote_format.clone();
    out.remote_size_bytes = c.remote_size_bytes;
    out.incomplete_reason = c.incomplete_reason.clone();
    out.tags_borrowed_from = c.tags_borrowed_from.clone();
    out.remote_available = c.remote_available;
    out.availability_checked_at = c.availability_checked_at;
    out.restricted = c.restricted.clone();
    out.booru_tags_fetched = c.booru_tags_fetched;
    out.remote_info_fetched = c.remote_info_fetched;
    out
}

/// `_candidate_to_dict`, as the cache's candidate (`tag_version` is derived from the URL when it
/// is written).
pub fn candidate_to_cache(c: &Candidate) -> cache::Candidate {
    let mut out = cache::Candidate::new(c.url.clone());
    out.source_name = c.source_name.clone();
    out.thumb_url = c.thumb_url.clone();
    out.similarity = c.similarity;
    out.similarity_measured = c.similarity_measured;
    out.width = c.width;
    out.height = c.height;
    out.engine = c.engine.clone();
    out.engine_tags = to_cache_tags(&c.engine_tags);
    out.booru_tags = to_cache_tags(&c.booru_tags);
    out.direct_file_url = c.direct_file_url.clone();
    out.preview_url = c.preview_url.clone();
    out.remote_format = c.remote_format.clone();
    out.remote_size_bytes = c.remote_size_bytes;
    out.incomplete_reason = c.incomplete_reason.clone();
    out.tags_borrowed_from = c.tags_borrowed_from.clone();
    out.remote_available = c.remote_available;
    out.availability_checked_at = c.availability_checked_at;
    out.restricted = c.restricted.clone();
    out.booru_tags_fetched = c.booru_tags_fetched;
    out.remote_info_fetched = c.remote_info_fetched;
    out
}

/// `apply_cached_result`: populates an entry from a cached result, mutating it in place the way a
/// real search would have. The candidates are the cache's; a result with some selects its stored
/// index.
pub fn apply_cached_result(entry: &mut Entry, cached: &CacheEntry) {
    entry.candidates = cached.candidates.iter().map(candidate_from_cache).collect();
    entry.status = from_cache_status(cached.status);
    entry.error_message = cached.error_message.clone();
    if !entry.candidates.is_empty() {
        entry.select_candidate(cached.selected_candidate_index);
    }
}

/// The dict `save_cached_result` writes, from an entry (`cached_at` is `now`).
pub fn cache_entry_for(entry: &Entry, now: f64) -> CacheEntry {
    let mut cached = CacheEntry::new(
        to_cache_status(entry.status),
        entry.candidates.iter().map(candidate_to_cache).collect(),
        now,
    );
    cached.error_message = entry.error_message.clone();
    cached.selected_candidate_index = entry.selected_candidate_index;
    cached
}
