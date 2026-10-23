//! Drives a real Chromium so Google Lens results can be read (`core/lens_browser.py`, the
//! lifecycle half): finding the browser, starting it with Lens's own profile, running one search
//! at a time from a thread of its own, warming a new profile up, noticing and waiting out a robot
//! check, and surviving a window the user closed.
//!
//! **The cut.** The per-search half of Python's `LensBrowser` (`_search_area` through `_one`, and
//! `fetch_results_payloads`) is 5.c's ([`SearchStep`]). This module owns everything around it, and
//! hands a step the page and the three things `_one` calls on `self`: [`StepCtx::warm_up`],
//! [`StepCtx::wait_out_challenge`] and [`StepCtx::is_challenged`]. The shipped step is
//! [`UnportedSearch`]; `LiveEngines` still holds `NoBrowser`, so `hatate batch` prints what it
//! printed before.
//!
//! **Threads.** Playwright's objects belong to the thread that made them, so Python hands every
//! request to one thread. The CDP client does not need that, but Chromium's profile, its window
//! and the "one search at a time" rule do, so the same shape is kept: a queue, one browser thread
//! with a runtime of its own, a job that carries its own reply.
//!
//! **Chromium without Playwright.** [`deps`] finds a browser; [`launch`] starts it with the argv
//! Playwright would have built, minus `--enable-automation`; [`page`] is the few Playwright calls
//! the lifecycle makes. `docs/lens-browser.md` has what could not be matched.

pub mod deps;
pub mod launch;
pub mod page;

use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chromiumoxide::Browser;
use chromiumoxide::handler::HandlerConfig;
use futures::{FutureExt, StreamExt};
use tokio::sync::{Notify, oneshot};

use self::deps::{
    BrowserEnv, DependencyStatus, LaunchFailure, check_dependencies, classify_launch_failure,
    find_chromium,
};
use self::launch::{LAUNCH_TIMEOUT, LaunchSpec, RunningChrome, WINDOW_CLASS, ensure_profile_dir};
use self::page::{LensPage, PageError};
use crate::clock::{BoxFuture, Clock, SystemClock};
use crate::engines::lens_search::{LensFetch, LensFetchError, LensPayloads};
use crate::error::ErrorKind;
use crate::image_prep::os_error_text;
use crate::pycompat;

/// `WARMUP_SEARCHES`: ordinary searches run once per session before the first image. A profile
/// with no history behind it is challenged nearly every time.
pub const WARMUP_SEARCHES: [&str; 2] = [
    "https://www.google.com/search?q=weather&hl=en",
    "https://www.google.com/search?q=news&hl=en",
];

/// `LensBrowser.SAFESEARCH_URL`.
pub const SAFESEARCH_URL: &str = "https://www.google.com/safesearch";

/// `CHALLENGE_WAIT_MS`: how long to leave a challenge on screen for the user to answer. It is
/// their window and their CAPTCHA; the only limit is that a batch left unattended should not wait
/// forever.
pub const CHALLENGE_WAIT_MS: u64 = 180_000;
pub const CHALLENGE_POLL_MS: u64 = 1000;
pub const RECAPTCHA_SELECTOR: &str = "iframe[src*='recaptcha'], iframe[title*='reCAPTCHA']";

/// How long `fetch` waits beyond the job's own timeout and the longest challenge wait before it
/// gives up on the browser thread (Python: `+ 120`).
pub const ANSWER_GRACE_SECONDS: f64 = 120.0;

/// How long `shutdown` waits for the thread to stop (Python: `join(timeout=15)`).
pub const SHUTDOWN_JOIN_SECONDS: u64 = 15;

/// What the window may do about the user (`core/desktop_attention.py`): kept off the taskbar
/// except while Google is asking for a robot check. The KWin implementation is not part of this
/// ticket; the shipped one does nothing, which is what Python does anywhere that is not Plasma.
pub trait WindowAttention: Send + Sync + 'static {
    /// `desktop_attention.quiet(window_class)`.
    fn quiet(&self, window_class: &str);
    /// `desktop_attention.ask_for_input(window_class)`.
    fn ask_for_input(&self, window_class: &str);
}

/// The shipped attention: none.
pub struct NoAttention;

impl WindowAttention for NoAttention {
    fn quiet(&self, _: &str) {}
    fn ask_for_input(&self, _: &str) {}
}

/// Where Lens goes, and what it does around the browser. [`LensSettings::shipped`] is the only
/// value the binary builds; a test builds its own, and that is the only way to point Lens
/// anywhere else (the URLs are not read from the environment or a config key).
#[derive(Clone)]
pub struct LensSettings {
    pub env: BrowserEnv,
    /// `PROFILE_DIR`.
    pub profile_dir: PathBuf,
    /// `SAFESEARCH_URL`.
    pub safesearch_url: String,
    /// `WARMUP_SEARCHES`.
    pub warmup_searches: Vec<String>,
    /// `CHALLENGE_WAIT_MS` and `CHALLENGE_POLL_MS`.
    pub challenge_wait_ms: u64,
    pub challenge_poll_ms: u64,
    /// The `+ 120` in `fetch`'s wait.
    pub answer_grace_seconds: f64,
    /// `page.wait_for_timeout` sleeps on this.
    pub waits: Arc<dyn Clock>,
    pub attention: Arc<dyn WindowAttention>,
    pub step: Arc<dyn SearchStep>,
    /// Flags a test adds to the launch (`--ozone-platform=headless`).
    pub extra_launch_args: Vec<String>,
}

impl LensSettings {
    /// The shipped settings: the real Google, the process environment, the real clock, the page
    /// steps 5.c has not ported yet.
    pub fn shipped(profile_dir: PathBuf) -> Self {
        LensSettings {
            env: BrowserEnv::from_process(),
            profile_dir,
            safesearch_url: SAFESEARCH_URL.to_owned(),
            warmup_searches: WARMUP_SEARCHES.iter().map(|u| (*u).to_owned()).collect(),
            challenge_wait_ms: CHALLENGE_WAIT_MS,
            challenge_poll_ms: CHALLENGE_POLL_MS,
            answer_grace_seconds: ANSWER_GRACE_SECONDS,
            waits: Arc::new(SystemClock::default()),
            attention: Arc::new(NoAttention),
            step: Arc::new(UnportedSearch),
            extra_launch_args: Vec::new(),
        }
    }
}

/// What `_one` raises: one of Python's `LensBrowserError`s, or whatever Playwright raised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepError {
    Lens(LensFetchError),
    Page(PageError),
}

impl From<LensFetchError> for StepError {
    fn from(error: LensFetchError) -> Self {
        StepError::Lens(error)
    }
}

impl From<PageError> for StepError {
    fn from(error: PageError) -> Self {
        StepError::Page(error)
    }
}

impl StepError {
    fn text(&self) -> String {
        match self {
            StepError::Lens(e) => e.python_text().to_owned(),
            StepError::Page(e) => e.message().to_owned(),
        }
    }
}

/// `_browser_gone`: whether Playwright is saying the window, page or browser is gone.
pub fn browser_gone(error: &StepError) -> bool {
    let is_target_closed = matches!(error, StepError::Page(e) if e.is_target_closed());
    is_target_closed || error.text().contains("has been closed")
}

/// `_first_line`: Playwright's messages run to a call log of many lines; the first one is the
/// part worth showing. `type_name` stands in when the message is empty.
pub fn first_line(text: &str, type_name: &str) -> String {
    let text = pycompat::strip(text);
    let line = text
        .split(['\n', '\r', '\u{0b}', '\u{0c}', '\u{1c}', '\u{1d}', '\u{1e}', '\u{85}', '\u{2028}', '\u{2029}'])
        .next()
        .unwrap_or("");
    if text.is_empty() {
        type_name.to_owned()
    } else {
        line.to_owned()
    }
}

fn step_error_to_fetch_error(error: StepError) -> LensFetchError {
    match error {
        StepError::Lens(e) => e,
        StepError::Page(e) => {
            let name = if e.is_target_closed() { "TargetClosedError" } else { "Error" };
            LensFetchError::Browser(first_line(e.message(), name))
        }
    }
}

impl LensFetchError {
    /// The text Python's exception carries.
    pub fn python_text(&self) -> &str {
        match self {
            LensFetchError::Unavailable { message, .. } => message,
            LensFetchError::ChallengeUnanswered(text) | LensFetchError::Browser(text) => text,
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            LensFetchError::Unavailable { .. } => ErrorKind::EngineUnavailable,
            LensFetchError::ChallengeUnanswered(_) => ErrorKind::EngineBlocked,
            LensFetchError::Browser(_) => ErrorKind::EngineFailed,
        }
    }

    /// What failed and what to do: no profile path, URL, token or OS text.
    pub fn user_message(&self) -> String {
        match self {
            LensFetchError::Unavailable { .. } => {
                "Google Lens could not start its browser. Install Chromium or Google Chrome, or \
                 close any other browser window that is using Lens's profile, and search again."
            }
            LensFetchError::ChallengeUnanswered(_) => {
                "Google asked for a robot check in the Lens browser window and it was not \
                 answered. Answer it when Lens asks again, or try later."
            }
            LensFetchError::Browser(_) => {
                "The Lens browser could not read the results for this image. Try again."
            }
        }
        .to_owned()
    }

    /// The raw text, which can hold the profile path or the browser's own output.
    pub fn detail(&self) -> String {
        self.python_text().to_owned()
    }
}

/// One request to the browser: `_Job` without the fields the browser thread fills in.
#[derive(Debug, Clone)]
pub struct JobRequest {
    pub image_bytes: Vec<u8>,
    pub filename: String,
    pub timeout: f64,
    pub whole_image: bool,
}

/// `_Job`.
struct Job {
    request: Arc<JobRequest>,
    retried: bool,
    reply: oneshot::Sender<Result<LensPayloads, LensFetchError>>,
}

impl Job {
    /// `job.error = error; job.done.set()`.
    fn fail(self, error: LensFetchError) {
        let _ = self.reply.send(Err(error));
    }
}

/// The per-search half, Python's `_one`: 5.c.
pub trait SearchStep: Send + Sync + 'static {
    fn one(&self, ctx: StepCtx, job: Arc<JobRequest>) -> BoxFuture<'_, Result<LensPayloads, StepError>>;
}

/// The step of a build that has not ported `_one` yet. It touches no page and no network.
pub struct UnportedSearch;

impl SearchStep for UnportedSearch {
    fn one(&self, _: StepCtx, _: Arc<JobRequest>) -> BoxFuture<'_, Result<LensPayloads, StepError>> {
        Box::pin(std::future::ready(Err(StepError::Lens(LensFetchError::Browser(
            "the Lens page steps are not part of this build yet".to_owned(),
        )))))
    }
}

/// What `_one` has to hand: the page, and `self._warm_up`, `self._wait_out_challenge` and
/// `self._is_challenged`.
#[derive(Clone)]
pub struct StepCtx {
    page: LensPage,
    core: Arc<Core>,
}

impl StepCtx {
    pub fn page(&self) -> &LensPage {
        &self.page
    }

    /// `_warm_up`: once per `LensBrowser`, set SafeSearch to Off and make two ordinary searches.
    pub async fn warm_up(&self) -> Result<(), StepError> {
        if self.core.warmed.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.turn_off_safesearch_blur().await;
        for url in &self.core.settings.warmup_searches {
            let visit = async {
                self.page.goto(url, 30_000).await?;
                self.page.wait_for_timeout(2000).await
            };
            if let Err(error) = visit.await {
                if browser_gone(&StepError::Page(error.clone())) {
                    return Err(error.into());
                }
                tracing::debug!("Warm-up navigation failed (harmless): {error}");
                continue;
            }
            // A fresh browser is the likeliest thing of all to be checked.
            self.wait_out_challenge("on first opening Google").await?;
        }
        Ok(())
    }

    /// `_turn_off_safesearch_blur`: set SafeSearch to Off, once, in this browser's own profile.
    ///
    /// Not about what results Google returns but about what its result tiles contain: with
    /// SafeSearch on "Blur" the thumbnail in each tile is a blurred placeholder that cannot be
    /// compared against. A nicety, never fatal: every failure is logged and carried on from.
    pub async fn turn_off_safesearch_blur(&self) {
        let outcome: Result<(), PageError> = async {
            let page = &self.page;
            page.goto(&self.core.settings.safesearch_url, 30_000).await?;
            page.wait_for_timeout(3000).await?;
            let radios = page.locator("[role=radio]");
            for index in 0..radios.count().await? {
                let radio = radios.nth(index);
                let text = radio.inner_text().await?;
                let first = text.split('\n').next().unwrap_or("");
                if pycompat::strip(first).to_lowercase() != "off" {
                    continue;
                }
                if radio.get_attribute("aria-checked").await?.as_deref() == Some("true") {
                    return Ok(()); // already set from a previous run
                }
                radio.click(8000).await?;
                page.wait_for_timeout(3000).await?;
                tracing::info!(
                    "Turned Google's SafeSearch blur off - its result thumbnails are blurred \
                     placeholders otherwise, which cannot be compared against"
                );
                return Ok(());
            }
            Ok(())
        }
        .await;
        if let Err(error) = outcome {
            tracing::debug!("Could not set SafeSearch ({error}) - carrying on");
        }
    }

    /// `_is_challenged`: Google's robot check, its `/sorry/` page or a reCAPTCHA in the page.
    pub async fn is_challenged(&self) -> Result<bool, PageError> {
        let read = async {
            if self.page.url().await.contains("/sorry/") {
                return Ok::<bool, PageError>(true);
            }
            Ok::<bool, PageError>(self.page.locator(RECAPTCHA_SELECTOR).count().await? > 0)
        };
        match read.await {
            Err(error) if browser_gone(&StepError::Page(error.clone())) => Err(error),
            Err(_) => Ok(false),
            Ok(challenged) => Ok(challenged),
        }
    }

    /// `_wait_out_challenge`: if Google is asking for a robot check, call the user and wait.
    ///
    /// Checked after every step that loads something, not only after the upload: a check arriving
    /// anywhere else sat in the window unannounced, with nothing asking for the user.
    pub async fn wait_out_challenge(&self, where_: &str) -> Result<(), StepError> {
        if !self.is_challenged().await? {
            return Ok(());
        }
        tracing::info!("Google is asking for a robot check ({where_}) - waiting for the user to answer it");
        let settings = &self.core.settings;
        // The one time the window does need the user.
        settings.attention.ask_for_input(WINDOW_CLASS);
        let outcome: Result<(), StepError> = async {
            self.page.bring_to_front().await?;
            let mut waited: u64 = 0;
            while self.is_challenged().await? {
                if waited >= settings.challenge_wait_ms {
                    return Err(StepError::Lens(LensFetchError::ChallengeUnanswered(
                        "the robot check in the browser window went unanswered".to_owned(),
                    )));
                }
                self.page.wait_for_timeout(settings.challenge_poll_ms).await?;
                waited += settings.challenge_poll_ms;
            }
            Ok(())
        }
        .await;
        settings.attention.quiet(WINDOW_CLASS);
        outcome?;
        tracing::info!("The robot check was answered - carrying on");
        Ok(())
    }
}

/// The jobs waiting for the browser thread (`queue.Queue[Optional[_Job]]`); `None` asks it to stop.
#[derive(Default)]
struct Queue {
    items: Mutex<VecDeque<Option<Job>>>,
    ready: Notify,
}

impl Queue {
    fn put(&self, item: Option<Job>) {
        self.items.lock().unwrap_or_else(|p| p.into_inner()).push_back(item);
        self.ready.notify_one();
    }

    fn get_nowait(&self) -> Option<Option<Job>> {
        self.items.lock().unwrap_or_else(|p| p.into_inner()).pop_front()
    }

    async fn get(&self) -> Option<Job> {
        loop {
            let notified = self.ready.notified();
            if let Some(item) = self.get_nowait() {
                return item;
            }
            notified.await;
        }
    }
}

/// Why the browser thread stopped: the exception Python's `_run` catches.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RunFailure {
    /// A launch exception.
    Launch(LaunchFailure),
    /// `LensBrowserError("Another Chromium is already using the Lens profile ...")`.
    ProfileInUse(String),
    /// Any other exception: the text it carries.
    Other(String),
}

impl RunFailure {
    fn text(&self) -> &str {
        match self {
            RunFailure::Launch(failure) => failure.text(),
            RunFailure::ProfileInUse(text) | RunFailure::Other(text) => text,
        }
    }
}

struct Core {
    settings: LensSettings,
    queue: Queue,
    /// `self._lock` and `self._thread`.
    thread: Mutex<Option<JoinHandle<()>>>,
    /// `self._warmed`.
    warmed: AtomicBool,
}

/// `LensBrowser`: a single Chromium, driven from its own thread.
#[derive(Clone)]
pub struct LensBrowser {
    core: Arc<Core>,
}

impl LensBrowser {
    /// The shipped browser, over Lens's own profile.
    pub fn new(profile_dir: PathBuf) -> Self {
        Self::with_settings(LensSettings::shipped(profile_dir))
    }

    pub fn with_settings(settings: LensSettings) -> Self {
        LensBrowser {
            core: Arc::new(Core {
                settings,
                queue: Queue::default(),
                thread: Mutex::new(None),
                warmed: AtomicBool::new(false),
            }),
        }
    }

    /// `start`.
    pub fn start(&self) {
        let mut thread = self.core.thread.lock().unwrap_or_else(|p| p.into_inner());
        if thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let core = Arc::clone(&self.core);
        *thread = Some(
            std::thread::Builder::new()
                .name("lens-browser".to_owned())
                .spawn(move || run_thread(core))
                .expect("a thread can be spawned"),
        );
    }

    /// `shutdown`: asks the thread to stop after the jobs ahead of the request, and waits for it
    /// for up to [`SHUTDOWN_JOIN_SECONDS`].
    pub fn shutdown(&self) {
        let mut thread = self.core.thread.lock().unwrap_or_else(|p| p.into_inner());
        let Some(handle) = thread.take() else { return };
        self.core.queue.put(None);
        let deadline = Instant::now() + Duration::from_secs(SHUTDOWN_JOIN_SECONDS);
        while !handle.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if handle.is_finished() {
            let _ = handle.join();
        }
    }

    /// `fetch`: uploads one image; returns its response bodies and result tiles.
    pub async fn fetch(
        &self,
        image_bytes: &[u8],
        filename: &str,
        timeout: f64,
        whole_image: bool,
    ) -> Result<LensPayloads, LensFetchError> {
        // Both the import and the browser, not just the import: a Chromium that was never
        // installed used to be found only by launching a browser and failing, thirty seconds into
        // the first image.
        let status = check_dependencies(&self.core.settings.env);
        if !status.ok() {
            return Err(LensFetchError::Unavailable {
                message: status.message,
                missing: status.missing,
            });
        }
        self.start();
        let (reply, answer) = oneshot::channel();
        self.core.queue.put(Some(Job {
            request: Arc::new(JobRequest {
                image_bytes: image_bytes.to_vec(),
                filename: filename.to_owned(),
                timeout,
                whole_image,
            }),
            retried: false,
            reply,
        }));
        // Generous: a challenge is answered by a person, and the browser side ends the job either way.
        let wait = self.core.settings.challenge_wait_ms as f64 / 1000.0
            + timeout
            + self.core.settings.answer_grace_seconds;
        let never = || LensFetchError::Browser("The browser never answered".to_owned());
        match tokio::time::timeout(Duration::from_secs_f64(wait.max(0.0)), answer).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) | Err(_) => Err(never()),
        }
    }
}

impl LensFetch for LensBrowser {
    fn fetch_results_payloads<'a>(
        &'a self,
        image: &'a [u8],
        filename: &'a str,
        timeout: f64,
        whole_image: bool,
    ) -> BoxFuture<'a, Result<LensPayloads, LensFetchError>> {
        Box::pin(self.fetch(image, filename, timeout, whole_image))
    }
}

static SHARED: OnceLock<LensBrowser> = OnceLock::new();

/// Python's module-level `_browser`, over the profile of the first caller.
pub fn shared(profile_dir: PathBuf) -> &'static LensBrowser {
    SHARED.get_or_init(|| LensBrowser::new(profile_dir))
}

/// The module-level `shutdown()`.
pub fn shutdown() {
    if let Some(browser) = SHARED.get() {
        browser.shutdown();
    }
}

/// `_unavailable`: a browser that would not start, said in the most useful terms. A Chromium that
/// is not there is recognised and replaced with what to install; anything else keeps its text.
fn unavailable(failure: &RunFailure, env: &BrowserEnv) -> LensFetchError {
    if let RunFailure::Launch(launch) = failure
        && let Some(DependencyStatus { missing, message }) = classify_launch_failure(launch, env)
    {
        return LensFetchError::Unavailable { message, missing };
    }
    LensFetchError::Unavailable {
        message: format!("The browser could not be started: {}", failure.text()),
        missing: None,
    }
}

/// `_drain`: fails every waiting job when the browser thread cannot run. Without this a caller
/// blocks until its own timeout for a browser that is never going to answer.
fn drain(core: &Core, failure: &RunFailure) {
    while let Some(item) = core.queue.get_nowait() {
        if let Some(job) = item {
            job.fail(unavailable(failure, &core.settings.env));
        }
    }
}

/// The thread body: a runtime of its own, and `_run` on it.
fn run_thread(core: Arc<Core>) {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
    match runtime {
        Ok(runtime) => runtime.block_on(run(&core)),
        Err(error) => drain(&core, &RunFailure::Other(error.to_string())),
    }
}

/// A started Chromium and the connection to it (`context`).
struct Session {
    chrome: Option<RunningChrome>,
    browser: Browser,
    gone: Arc<AtomicBool>,
    events: tokio::task::JoinHandle<()>,
}

impl Session {
    /// `context.close()`, whatever state the browser is in.
    async fn close(mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(5), self.browser.close()).await;
        if let Some(mut chrome) = self.chrome.take() {
            let _ = tokio::task::spawn_blocking(move || chrome.finish(Duration::from_secs(5))).await;
        }
        self.events.abort();
    }
}

/// `_run`.
async fn run(core: &Arc<Core>) {
    let mut retry: Option<Job> = None;
    let failure = loop {
        let session = match launch_session(core).await {
            Ok(session) => session,
            Err(failure) => break failure,
        };
        let outcome = serve(core, &session, retry.take()).await;
        session.close().await;
        match outcome {
            Served::Stopped => return,
            // The window was closed under us. A fresh one, and the search it interrupted runs again there.
            Served::Interrupted(job) => retry = Some(job),
            Served::Failed { failure, first } => {
                retry = first;
                break failure;
            }
        }
    };
    tracing::error!("Lens browser thread stopped: {}", failure.text());
    if let Some(job) = retry {
        job.fail(unavailable(&failure, &core.settings.env));
    }
    drain(core, &failure);
}

/// `_launch` and `_launch_context`.
async fn launch_session(core: &Arc<Core>) -> Result<Session, RunFailure> {
    let settings = &core.settings;
    ensure_profile_dir(&settings.profile_dir).map_err(|error| {
        RunFailure::Other(os_error_text(&error, &settings.profile_dir))
    })?;
    tracing::info!("Starting Chromium for Google Lens");
    let in_use = |text: &str| text.contains("already in use") || text.contains("Opening in existing browser");
    let profile_in_use = || {
        RunFailure::ProfileInUse(format!(
            "Another Chromium is already using the Lens profile at {}. Close it - or, if a \
             previous run left it behind, quit that browser window - and search again.",
            settings.profile_dir.display()
        ))
    };

    let Some(found) = find_chromium(&settings.env) else {
        return Err(RunFailure::Launch(LaunchFailure::Spawn {
            kind: std::io::ErrorKind::NotFound,
            text: "no Chromium found".to_owned(),
        }));
    };
    let spec = LaunchSpec {
        executable: found.executable,
        profile_dir: settings.profile_dir.clone(),
        extra_args: settings.extra_launch_args.clone(),
    };
    let chrome = tokio::task::spawn_blocking(move || launch::launch(&spec, LAUNCH_TIMEOUT))
        .await
        .map_err(|join| RunFailure::Other(join.to_string()))?
        .map_err(|failure| {
            if in_use(failure.text()) {
                profile_in_use()
            } else {
                RunFailure::Launch(failure)
            }
        })?;

    let config = HandlerConfig {
        // Never: the browser checks every certificate, and no setting turns that off.
        ignore_https_errors: false,
        viewport: None,
        ..HandlerConfig::default()
    };
    let (browser, mut handler) = Browser::connect_with_config(chrome.ws_url().to_owned(), config)
        .await
        .map_err(|error| {
            let text = error.to_string();
            if in_use(&text) {
                profile_in_use()
            } else {
                RunFailure::Launch(LaunchFailure::Connect { text })
            }
        })?;
    let gone = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&gone);
    let events = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
        flag.store(true, Ordering::SeqCst);
    });
    Ok(Session {
        chrome: Some(chrome),
        browser,
        gone,
        events,
    })
}

enum Served {
    /// Asked to stop.
    Stopped,
    /// The window was closed under a job, which is to be run again in a fresh one.
    Interrupted(Job),
    /// The session could not be used at all; `first` is the job it was handed, if it had one.
    Failed { failure: RunFailure, first: Option<Job> },
}

/// `_serve`: runs jobs until asked to stop, or until the browser window is closed under us.
///
/// CONFIRMED in Python: a user closing the window after an unanswered robot check left every later
/// search failing on "Target page, context or browser has been closed", as a raw traceback, until
/// restart.
async fn serve(core: &Arc<Core>, session: &Session, first: Option<Job>) -> Served {
    let page = match first_page(session).await {
        Ok(page) => page,
        Err(error) => {
            return Served::Failed {
                failure: RunFailure::Other(error.message().to_owned()),
                first,
            };
        }
    };
    let page = LensPage::new(page, Arc::clone(&session.gone), Arc::clone(&core.settings.waits));
    if let Err(error) = page.emulate().await {
        return Served::Failed {
            failure: RunFailure::Other(error.message().to_owned()),
            first,
        };
    }
    let ctx = StepCtx {
        page,
        core: Arc::clone(core),
    };
    let mut job = first;
    loop {
        let current = match job.take() {
            Some(job) => job,
            None => match core.queue.get().await {
                Some(job) => job,
                None => return Served::Stopped,
            },
        };
        let attempt = AssertUnwindSafe(core.settings.step.one(ctx.clone(), Arc::clone(&current.request)))
            .catch_unwind()
            .await;
        let result = attempt.unwrap_or_else(|panic| {
            let text = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".to_owned());
            Err(StepError::Lens(LensFetchError::Browser(first_line(&text, "Error"))))
        });
        match result {
            Ok(payloads) => {
                let _ = current.reply.send(Ok(payloads));
            }
            Err(error) => {
                if browser_gone(&error) && !current.retried {
                    let mut current = current;
                    current.retried = true;
                    tracing::info!(
                        "The Lens browser window was closed - opening a new one and running this search again"
                    );
                    return Served::Interrupted(current);
                }
                current.fail(step_error_to_fetch_error(error));
            }
        }
    }
}

/// `context.pages[0] if context.pages else context.new_page()`.
async fn first_page(session: &Session) -> Result<chromiumoxide::Page, PageError> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let pages = session
            .browser
            .pages()
            .await
            .map_err(|e| PageError::other("BrowserContext.pages", e))?;
        if let Some(page) = pages.into_iter().next() {
            return Ok(page);
        }
        if Instant::now() >= deadline {
            return session
                .browser
                .new_page("about:blank")
                .await
                .map_err(|e| PageError::other("BrowserContext.new_page", e));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_is_the_first_line_of_the_stripped_text_or_the_type_name() {
        assert_eq!(first_line("  one\ntwo\n", "Error"), "one");
        assert_eq!(first_line("a\r\nb", "Error"), "a");
        assert_eq!(first_line("a\u{2028}b", "Error"), "a");
        assert_eq!(first_line(" \n ", "TargetClosedError"), "TargetClosedError");
        assert_eq!(first_line("", "Error"), "Error");
    }

    #[test]
    fn a_closed_target_is_gone_and_so_is_any_text_that_says_closed() {
        let closed = StepError::Page(PageError::target_closed("Page.goto"));
        assert!(browser_gone(&closed));
        assert!(browser_gone(&StepError::Lens(LensFetchError::Browser(
            "Page.evaluate: the page has been closed".to_owned()
        ))));
        assert!(!browser_gone(&StepError::Page(PageError::other(
            "Page.goto",
            "Timeout 30000ms exceeded."
        ))));
    }

    #[test]
    fn the_error_shape_keeps_paths_and_os_text_out_of_the_user_message() {
        let secret = "/home/someone/.config/hatate/lens_profile";
        for error in [
            LensFetchError::Unavailable {
                message: format!("The browser could not be started: profile at {secret}: os error 13"),
                missing: None,
            },
            LensFetchError::ChallengeUnanswered(format!("{secret} os error 2")),
            LensFetchError::Browser(format!("failed at https://x/?key=hunter2 {secret} os error 2")),
        ] {
            let shown = error.user_message();
            assert!(!shown.contains(secret) && !shown.contains("os error") && !shown.contains("hunter2"));
            assert!(error.detail().contains("os error"));
        }
        assert_eq!(
            LensFetchError::Browser(String::new()).kind(),
            ErrorKind::EngineFailed
        );
    }
}
