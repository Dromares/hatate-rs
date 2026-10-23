//! The engine half of Google Lens (`core/google_lens.py`): `search` over an injected browser, the
//! pacing and stand-down state, and the alert that says Lens cannot run at all.
//!
//! **The cut.** The browser (`core/lens_browser.py`) is behind [`LensFetch`]: it uploads the
//! picture from inside a page and hands back the response bodies and the rendered exact-match
//! tiles. This module owns everything around that call. Until the CDP browser lands (5.b, 5.c) the
//! shipped fetch is [`NoBrowser`], which answers "unavailable" exactly as a Python install
//! without Playwright does, so `hatate batch` output does not change.
//!
//! **State.** Python keeps `_blocked`, `_consecutive_failures`, `_resting_until` and `_next_turn`
//! in module globals; here they are the client's, and clones share them. Time is the injected
//! [`Clock`]'s, so a test (or the differential) runs a 30-minute rest without waiting for it.
//!
//! **Connection state.** The client reports [`LensConnection`] to a sink as it learns it. `Ready`
//! (a search came back) and `Blocked` (it stood down, or is resting) are reachable here;
//! `NotReachable` and `WaitingForLens` belong to the browser driver (5.b).

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::google_lens::{
    GoogleLensMatch, SIMILARITY_FLOOR, SIMILARITY_START, SIMILARITY_STEP, parse_exact_matches,
    parse_exact_tiles, parse_lens_payload,
};
use super::lens_prep::{self, PrepareError};
use super::lens_rank::{
    LensSites, MAX_RESULTS, NoSites, RESERVED_VISUAL_SLOTS, Unexpected, effective_timeout,
    keep_taggable_first, recover_rule34_searches, resolve_subreddits, taggable_first,
};
use crate::clock::{BoxFuture, Clock, SystemClock};
use crate::engine_alerts::{EngineAlert, EngineAlerts};
use crate::engine_runner::LensError;
use crate::engines::client::{OnTick, ProgressTicker};
use crate::engines::saucenao::ShouldStop;
use crate::error::ErrorKind;
use crate::image_prep::os_error_text;

/// `MAX_CONSECUTIVE_FAILURES`: how many images in a row may fail before the engine stands down.
pub const MAX_CONSECUTIVE_FAILURES: usize = 3;

/// `CHALLENGE_REST_SECONDS`: after a robot check goes unanswered, Lens rests this long before
/// asking Google again.
pub const CHALLENGE_REST_SECONDS: f64 = 30.0 * 60.0;

/// `MIN_SECONDS_BETWEEN_SEARCHES`: the least time between the starts of two Lens searches,
/// however they were started.
pub const MIN_SECONDS_BETWEEN_SEARCHES: f64 = 45.0;

/// `ENGINE_NAME`.
pub const ENGINE_NAME: &str = "Google Lens";

/// `UNKNOWN_ALERT_KEY`.
pub const UNKNOWN_ALERT_KEY: &str = "google-lens:browser-unavailable";

/// What the shipped fetch says: the text Python's `LensBrowserUnavailable` carries when the
/// browser cannot be started. In Python it is the install command for the interpreter running the
/// app, which has no counterpart here.
pub const LENS_UNAVAILABLE_MESSAGE: &str =
    "No browser engine is available to render the results page in this build.";

/// `lens_browser.MISSING_PLAYWRIGHT` / `MISSING_CHROMIUM`: which of Lens's two optional
/// dependencies is absent. The Rust browser driver decides which of them it can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LensMissing {
    Playwright,
    Chromium,
}

impl LensMissing {
    /// `ALERT_KEYS`: one key per condition, so the once-per-session rule counts the missing
    /// dependency rather than the images that ran into it.
    pub fn alert_key(self) -> &'static str {
        match self {
            LensMissing::Playwright => "google-lens:playwright-not-installed",
            LensMissing::Chromium => "google-lens:chromium-not-installed",
        }
    }
}

/// What `lens_browser.fetch_results_payloads` raises.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LensFetchError {
    /// `LensBrowserUnavailable(message, missing)`.
    Unavailable {
        message: String,
        missing: Option<LensMissing>,
    },
    /// `LensChallengeUnanswered`.
    ChallengeUnanswered(String),
    /// `LensBrowserError`.
    Browser(String),
}

/// What `fetch_results_payloads` returns: the response bodies (several pages are loaded per
/// search) and the rendered exact-match tiles (a JSON list of `{"text", "thumb"}` objects).
#[derive(Debug, Clone, PartialEq)]
pub struct LensPayloads {
    pub payloads: Vec<String>,
    pub tiles: Value,
}

/// `lens_browser.fetch_results_payloads(image, filename, timeout, whole_image=...)`.
pub trait LensFetch: Send + Sync + 'static {
    fn fetch_results_payloads<'a>(
        &'a self,
        image: &'a [u8],
        filename: &'a str,
        timeout: f64,
        whole_image: bool,
    ) -> BoxFuture<'a, Result<LensPayloads, LensFetchError>>;
}

/// The browser of a build that has none.
pub struct NoBrowser;

impl LensFetch for NoBrowser {
    fn fetch_results_payloads<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: f64,
        _: bool,
    ) -> BoxFuture<'a, Result<LensPayloads, LensFetchError>> {
        Box::pin(std::future::ready(Err(LensFetchError::Unavailable {
            message: LENS_UNAVAILABLE_MESSAGE.to_owned(),
            missing: None,
        })))
    }
}

/// `LensConnection`: what the client knows about Lens, for a UI to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LensConnection {
    /// The browser cannot be reached.
    NotReachable,
    /// A search is under way and Lens has not answered.
    WaitingForLens,
    /// Lens answered the last search.
    Ready,
    /// Lens stood down for the run, or is resting after an unanswered robot check.
    Blocked,
}

/// Where [`LensConnection`] changes go.
pub type ConnectionSink = Arc<dyn Fn(LensConnection) + Send + Sync + 'static>;

#[derive(Default)]
struct State {
    /// `_blocked`.
    blocked: bool,
    /// `len(_consecutive_failures)`.
    failures: usize,
    /// `_resting_until` (the client's monotonic clock).
    resting_until: Option<f64>,
    /// `_next_turn` (the client's monotonic clock).
    next_turn: Option<f64>,
    connection: Option<LensConnection>,
}

/// Google Lens as the runner asks it.
#[derive(Clone)]
pub struct LensClient {
    fetch: Arc<dyn LensFetch>,
    sites: Arc<dyn LensSites>,
    clock: Arc<dyn Clock>,
    alerts: Arc<EngineAlerts>,
    sink: Option<ConnectionSink>,
    appimage: bool,
    state: Arc<Mutex<State>>,
}

impl LensClient {
    /// The shipped client: no browser, no site lookups, the real clock.
    pub fn new() -> Self {
        Self::with_fetch(Arc::new(NoBrowser))
    }

    /// A client over `fetch`.
    pub fn with_fetch(fetch: Arc<dyn LensFetch>) -> Self {
        LensClient {
            fetch,
            sites: Arc::new(NoSites),
            clock: Arc::new(SystemClock::default()),
            alerts: Arc::new(EngineAlerts::new()),
            sink: None,
            appimage: false,
            state: Arc::default(),
        }
    }

    /// The clock the rest and the pacing are measured on.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// The rule34 and Reddit lookups `search` makes after a page is parsed.
    #[must_use]
    pub fn with_sites(mut self, sites: Arc<dyn LensSites>) -> Self {
        self.sites = sites;
        self
    }

    /// Where alerts go (shared with whoever shows them).
    #[must_use]
    pub fn with_alerts(mut self, alerts: Arc<EngineAlerts>) -> Self {
        self.alerts = alerts;
        self
    }

    /// Where connection changes go.
    #[must_use]
    pub fn with_connection_sink(mut self, sink: ConnectionSink) -> Self {
        self.sink = Some(sink);
        self
    }

    /// `running_as_appimage()`, which changes the text of the "cannot run" alert.
    #[must_use]
    pub fn running_as_appimage(mut self, yes: bool) -> Self {
        self.appimage = yes;
        self
    }

    pub fn alerts(&self) -> &Arc<EngineAlerts> {
        &self.alerts
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// What the client last reported, if it has reported anything.
    pub fn connection(&self) -> Option<LensConnection> {
        self.lock().connection
    }

    fn report(&self, connection: LensConnection) {
        self.lock().connection = Some(connection);
        if let Some(sink) = &self.sink {
            sink(connection);
        }
    }

    /// `is_blocked`: stood down, or resting after a robot check.
    pub fn is_blocked(&self) -> bool {
        self.lock().blocked || self.is_resting()
    }

    /// `is_resting`.
    pub fn is_resting(&self) -> bool {
        let monotonic = self.clock.monotonic();
        self.lock()
            .resting_until
            .is_some_and(|until| monotonic < until)
    }

    /// `resting_until`: when the rest ends, as a local clock time, e.g. "21:41"; empty when it is
    /// not resting.
    pub fn resting_until(&self) -> String {
        let monotonic = self.clock.monotonic();
        let until = match self.lock().resting_until {
            Some(until) if monotonic < until => until,
            _ => return String::new(),
        };
        self.clock
            .local_hhmm(self.clock.wall() + (until - monotonic))
    }

    /// `reset_blocked_flag`: called whenever a search starts. Clears a stand-down, but not a rest
    /// after an unanswered robot check, which is the point of it.
    pub fn reset_blocked_flag(&self) {
        let mut state = self.lock();
        state.blocked = false;
        state.failures = 0;
    }

    /// `end_rest`: lets Lens ask Google again now.
    pub fn end_rest(&self) {
        let mut state = self.lock();
        state.resting_until = None;
        state.next_turn = None;
    }

    /// `wait_for_turn`: holds a search until [`MIN_SECONDS_BETWEEN_SEARCHES`] have passed since
    /// the last one started. The turn is claimed before waiting, so two searches arriving
    /// together are spaced out rather than both let go. `should_stop` is polled every second.
    pub async fn wait_for_turn(&self, on_tick: Option<OnTick>, should_stop: Option<ShouldStop>) {
        let now = self.clock.monotonic();
        let start_at = {
            let mut state = self.lock();
            // `max(now, _next_turn or now)`
            let start_at = now.max(state.next_turn.filter(|t| *t != 0.0).unwrap_or(now));
            state.next_turn = Some(start_at + MIN_SECONDS_BETWEEN_SEARCHES);
            start_at
        };
        let wait = start_at - now;
        if wait <= 0.0 {
            return;
        }
        tracing::info!("Pacing Google Lens: waiting {wait:.0}s so Google isn't asked too often");
        loop {
            let left = start_at - self.clock.monotonic();
            if left <= 0.0 || should_stop.as_ref().is_some_and(|stop| stop()) {
                return;
            }
            if let Some(tick) = &on_tick {
                tick("Pacing Google Lens", left, wait);
            }
            self.clock.sleep(left.min(1.0)).await;
        }
    }

    /// `announce_unavailable`: asks for a user-visible warning that Lens cannot run at all. Only
    /// for the unavailable case, never for an ordinary failed search.
    pub fn announce_unavailable(&self, message: &str, missing: Option<LensMissing>) -> bool {
        let body = match (missing, self.appimage) {
            (Some(_), true) => "Google Lens was skipped: the Playwright and Chromium bundled in \
                this AppImage could not be found, so no browser could be started. Every image in \
                this run was searched by the other engines only."
                .to_owned(),
            (Some(LensMissing::Playwright), false) => "Google Lens was skipped: the Playwright \
                package it needs is not installed, so no browser could be started. Every image in \
                this run was searched by the other engines only."
                .to_owned(),
            (Some(LensMissing::Chromium), false) => "Google Lens was skipped: Playwright is \
                installed but the Chromium browser it drives is not, so no browser could be \
                started. Every image in this run was searched by the other engines only."
                .to_owned(),
            (None, _) => {
                format!("Google Lens was skipped: its browser could not be started.\n\n{message}")
            }
        };
        self.alerts.raise_alert(&EngineAlert {
            key: missing
                .map_or(UNKNOWN_ALERT_KEY, LensMissing::alert_key)
                .to_owned(),
            engine: ENGINE_NAME.to_owned(),
            title: "Google Lens cannot run".to_owned(),
            body,
            // The exception's own text is the install command for the missing piece.
            remedy: if missing.is_some() {
                message.to_owned()
            } else {
                String::new()
            },
        })
    }

    /// `_latch`: stands the engine down until `reset_blocked_flag`.
    fn latch(&self) {
        self.lock().blocked = true;
        self.report(LensConnection::Blocked);
    }

    /// `_count_failure`: reports a failed image, standing the engine down after a run of them.
    fn count_failure(&self, message: &str) -> LensError {
        let failures = {
            let mut state = self.lock();
            state.failures += 1;
            state.failures
        };
        if failures >= MAX_CONSECUTIVE_FAILURES {
            self.latch();
            return LensError::Blocked(format!(
                "{message} That has now happened to {failures} images in a row, so Google Lens is \
                 standing down for the rest of this run. Start the search again to retry it, or \
                 use the Google Images engine, which reaches Google through the Cloud Vision API \
                 instead."
            ));
        }
        LensError::Failed(message.to_owned())
    }

    /// `search`: uploads an image to Google and reads the rendered Lens results.
    pub async fn search(
        &self,
        image_path: &Path,
        timeout: f64,
        on_tick: Option<OnTick>,
        whole_image: bool,
    ) -> Result<Vec<GoogleLensMatch>, LensError> {
        let owned = image_path.to_owned();
        let prepared = tokio::task::spawn_blocking(move || lens_prep::prepare_for_lens(&owned))
            .await
            .map_err(|join| LensError::Unexpected(join.to_string()))?;
        let upload = match prepared {
            Ok(upload) => upload,
            Err(PrepareError::Unreadable { error, .. }) => {
                tracing::error!(
                    "Could not read {} for the Lens upload: {}",
                    image_path.display(),
                    os_error_text(&error, image_path)
                );
                return Err(LensError::Failed(format!(
                    "Could not read image: {}",
                    os_error_text(&error, image_path)
                )));
            }
            // Not caught by Python's `except OSError`: an ordinary failed image, with no latch
            // and no count.
            Err(PrepareError::Refused(message)) => return Err(LensError::Failed(message)),
        };

        tracing::debug!(
            "Uploading and rendering Lens results for {}",
            image_path.display()
        );
        let budget = effective_timeout(timeout);
        let fetched = {
            let _ticker = ProgressTicker::start("Waiting on Google Lens", budget, on_tick);
            self.fetch
                .fetch_results_payloads(&upload.data, &upload.filename, budget, whole_image)
                .await
        };
        let LensPayloads { payloads, tiles } = match fetched {
            Ok(fetched) => fetched,
            Err(LensFetchError::Unavailable { message, missing }) => {
                // Told once per session, by whoever subscribed: a stood-down engine is
                // indistinguishable from one that searched and found nothing.
                self.announce_unavailable(&message, missing);
                self.latch();
                return Err(LensError::Unavailable(message));
            }
            Err(LensFetchError::ChallengeUnanswered(exc)) => {
                self.lock().resting_until = Some(self.clock.monotonic() + CHALLENGE_REST_SECONDS);
                let until = self.resting_until();
                tracing::error!(
                    "Google Lens challenge went unanswered: {exc} - resting until {until}"
                );
                self.report(LensConnection::Blocked);
                // A rest, not a stand-down as well: the message promises Lens back at a time, and
                // a batch runs for hours.
                return Err(LensError::Blocked(format!(
                    "Google asked to check you aren't a robot in the browser window and the \
                     check wasn't answered, so Lens is resting until {until} - asking again \
                     straight away just brings another check. Turn Google Lens off under \
                     Settings > Engine to stop it altogether."
                )));
            }
            Err(LensFetchError::Browser(exc)) => {
                tracing::error!(
                    "Google Lens browser failed for {}: {exc}",
                    image_path.display()
                );
                return Err(
                    self.count_failure(&format!("the browser could not read the results ({exc})"))
                );
            }
        };

        if payloads.is_empty() {
            return Err(self.count_failure(
                "Google returned a results page with nothing in it. If the browser window \
                 showed \"can't read file\", this image is one Google will not accept.",
            ));
        }

        let sites = Arc::clone(&self.sites);
        let data = upload.data.clone();
        let ranked =
            tokio::task::spawn_blocking(move || rank(&payloads, &tiles, &data, sites.as_ref()))
                .await
                .map_err(|join| LensError::Unexpected(join.to_string()))?
                .map_err(|Unexpected(message)| LensError::Unexpected(message))?;

        self.lock().failures = 0;
        self.report(LensConnection::Ready);
        Ok(ranked)
    }
}

impl Default for LensClient {
    fn default() -> Self {
        Self::new()
    }
}

/// The part of `search` after the pages are in hand: parse them, put exact matches ahead of
/// visual ones and sites that give tags ahead of the rest, score by position, and cut.
///
/// Several pages are loaded per search (the first results, the Visual matches tab, and whatever
/// confirming the explicit-results notice loads next), so the one that parses best wins. Choosing
/// by size instead threw real matches away.
pub fn rank(
    payloads: &[String],
    tiles: &Value,
    image_bytes: &[u8],
    sites: &dyn LensSites,
) -> Result<Vec<GoogleLensMatch>, Unexpected> {
    let mut visual: Vec<GoogleLensMatch> = Vec::new();
    let mut exact: Vec<GoogleLensMatch> = Vec::new();
    let add_exact = |exact: &mut Vec<GoogleLensMatch>, m: GoogleLensMatch| {
        if exact.iter().all(|e| e.url != m.url) {
            exact.push(m);
        }
    };
    // The rendered tiles first: they carry the same exact matches the response body does, plus
    // the thumbnail and dimensions the body leaves out. The body is the fallback.
    for m in parse_exact_tiles(tiles) {
        add_exact(&mut exact, m);
    }
    for payload in payloads {
        let parsed = parse_lens_payload(payload);
        if parsed.len() > visual.len() {
            visual = parsed;
        }
        for m in parse_exact_matches(payload) {
            add_exact(&mut exact, m);
        }
    }
    for m in recover_rule34_searches(tiles, image_bytes, sites)? {
        if exact.iter().all(|e| e.url != m.url) {
            exact.insert(0, m);
        }
    }

    visual.retain(|m| exact.iter().all(|e| e.url != m.url));
    let (exact, visual) = resolve_subreddits(exact, visual, sites);
    let visual_start = exact.len();
    let mut matches = taggable_first(exact)?;
    matches.extend(taggable_first(visual)?);
    for (rank, m) in matches.iter_mut().enumerate() {
        m.similarity = (SIMILARITY_START - rank as f64 * SIMILARITY_STEP).max(SIMILARITY_FLOOR);
    }
    let matches = keep_taggable_first(matches, MAX_RESULTS, visual_start, RESERVED_VISUAL_SLOTS)?;
    tracing::debug!(
        "Google Lens parsed {} response(s) into {} match(es) ({visual_start} exact)",
        payloads.len(),
        matches.len()
    );
    Ok(matches)
}

impl LensError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            LensError::Unavailable(_) => ErrorKind::EngineUnavailable,
            LensError::Blocked(_) => ErrorKind::EngineBlocked,
            LensError::Failed(_) => ErrorKind::EngineFailed,
            LensError::Unexpected(_) => ErrorKind::Unexpected,
        }
    }

    /// What failed and what to do: no path, URL, profile directory or OS text.
    pub fn user_message(&self) -> String {
        match self {
            LensError::Unavailable(_) => {
                "Google Lens could not start a browser, so it was skipped. Install or repair the \
                 browser it needs and start the search again."
            }
            LensError::Blocked(_) => {
                "Google Lens stood down: Google asked for a robot check that was not answered, or \
                 several images in a row failed. Try again later, or turn Google Lens off in \
                 Settings."
            }
            LensError::Failed(_) => {
                "Google Lens could not search this image. Try again, or use the other engines."
            }
            LensError::Unexpected(_) => {
                "Google Lens hit an unexpected error. Try again, or use the other engines."
            }
        }
        .to_owned()
    }

    /// The raw text Python's exception carries (it can hold a path, a URL or OS text).
    pub fn detail(&self) -> String {
        match self {
            LensError::Unavailable(m)
            | LensError::Blocked(m)
            | LensError::Failed(m)
            | LensError::Unexpected(m) => m.clone(),
        }
    }
}
