//! The Lens browser lifecycle over a real Chromium (DAN-2164): launch, the page it opens, the
//! warm-up, a robot check that clears and one that does not, a window closed under a search, a
//! profile already in use, a launch that fails with jobs waiting, shutdown with a job ahead of it.
//!
//! These start a browser, so they are `#[ignore]`d and run with `--include-ignored`. The browser
//! is `$HATATE_TEST_CHROMIUM` or the first of `chromium`, `chromium-browser`,
//! `google-chrome-stable`, `google-chrome` on `PATH`; `$HATATE_TEST_BROWSER_ARGS` adds flags
//! (`--ozone-platform=headless` where there is no display). Each test uses a fresh profile in a
//! temp `HOME`, and the site is a fake on 127.0.0.1: nothing here reaches Google.

mod common;

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::Site;
use hatate_core::clock::{BoxFuture, Clock};
use hatate_core::engines::lens_search::{LensFetchError, LensMissing, LensPayloads};
use hatate_core::lens_browser::deps::BrowserEnv;
use hatate_core::lens_browser::page::PageError;
use hatate_core::lens_browser::{
    JobRequest, LensBrowser, LensSettings, SearchStep, StepCtx, StepError, WindowAttention,
};
use serde_json::json;

type StepFn = dyn Fn(StepCtx, Arc<JobRequest>) -> BoxFuture<'static, Result<LensPayloads, StepError>>
    + Send
    + Sync;

struct FnStep(Box<StepFn>);

impl SearchStep for FnStep {
    fn one(
        &self,
        ctx: StepCtx,
        job: Arc<JobRequest>,
    ) -> BoxFuture<'_, Result<LensPayloads, StepError>> {
        (self.0)(ctx, job)
    }
}

/// Every wait asked for, and a hook run on each (the test's way of moving the site on).
struct HookClock {
    waits: Mutex<Vec<f64>>,
    hook: Box<dyn Fn(usize) -> BoxFuture<'static, ()> + Send + Sync>,
}

impl Clock for HookClock {
    fn wall(&self) -> f64 {
        0.0
    }
    fn monotonic(&self) -> f64 {
        0.0
    }
    fn sleep(&self, seconds: f64) -> BoxFuture<'_, ()> {
        let n = {
            let mut waits = self.waits.lock().unwrap();
            waits.push(seconds);
            waits.len()
        };
        let hook = (self.hook)(n);
        // A little real time, so what the page does meanwhile (a click's request) lands.
        Box::pin(async move {
            hook.await;
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        })
    }
}

#[derive(Default)]
struct Calls(Mutex<Vec<String>>);

impl WindowAttention for Calls {
    fn quiet(&self, class: &str) {
        self.0.lock().unwrap().push(format!("quiet {class}"));
    }
    fn ask_for_input(&self, class: &str) {
        self.0.lock().unwrap().push(format!("ask {class}"));
    }
}

fn test_browser() -> PathBuf {
    if let Some(path) = std::env::var_os("HATATE_TEST_CHROMIUM") {
        return PathBuf::from(path);
    }
    for name in [
        "chromium",
        "chromium-browser",
        "google-chrome-stable",
        "google-chrome",
    ] {
        for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    panic!("no Chromium: set HATATE_TEST_CHROMIUM");
}

struct Rig {
    dir: tempfile::TempDir,
    site: Site,
    waits: Arc<HookClock>,
    attention: Arc<Calls>,
}

impl Rig {
    async fn new(hook: impl Fn(usize) -> BoxFuture<'static, ()> + Send + Sync + 'static) -> Rig {
        let dir = tempfile::Builder::new()
            .prefix("hatate-dan2164-")
            .tempdir()
            .unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(test_browser(), bin.join("chromium")).unwrap();
        Rig {
            site: Site::start().await,
            waits: Arc::new(HookClock {
                waits: Mutex::new(Vec::new()),
                hook: Box::new(hook),
            }),
            attention: Arc::new(Calls::default()),
            dir,
        }
    }

    fn profile(&self) -> PathBuf {
        self.dir
            .path()
            .join("home/.config/hatate-linux/lens_profile")
    }

    fn settings(&self, step: Arc<dyn SearchStep>) -> LensSettings {
        let mut settings = LensSettings::shipped(self.profile());
        settings.env = BrowserEnv {
            home: Some(self.dir.path().join("home")),
            path: Some(OsString::from(self.dir.path().join("bin"))),
            ..BrowserEnv::default()
        };
        settings.safesearch_url = self.site.url("/safesearch");
        settings.warmup_searches = vec![
            self.site.url("/search?q=weather&hl=en"),
            self.site.url("/search?q=news&hl=en"),
        ];
        settings.waits = Arc::clone(&self.waits) as Arc<dyn Clock>;
        settings.attention = Arc::clone(&self.attention) as Arc<dyn WindowAttention>;
        settings.step = step;
        settings.extra_launch_args = std::env::var("HATATE_TEST_BROWSER_ARGS")
            .map(|v| v.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default();
        settings
    }
}

fn payload(text: &str) -> LensPayloads {
    LensPayloads {
        payloads: vec![text.to_owned()],
        tiles: json!([]),
    }
}

fn step(
    f: impl Fn(StepCtx, Arc<JobRequest>) -> BoxFuture<'static, Result<LensPayloads, StepError>>
    + Send
    + Sync
    + 'static,
) -> Arc<dyn SearchStep> {
    Arc::new(FnStep(Box::new(f)))
}

async fn fetch(browser: &LensBrowser) -> Result<LensPayloads, LensFetchError> {
    browser.fetch(b"image", "a.jpg", 5.0, true).await
}

/// The browser's own processes started for `profile`, by their command lines.
fn chrome_command_lines(profile: &Path) -> Vec<Vec<String>> {
    let needle = format!("--user-data-dir={}", profile.display());
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<String> = raw
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        if args.contains(&needle) && !args.iter().any(|a| a.starts_with("--type=")) {
            found.push(args);
        }
    }
    found
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn the_page_reads_as_the_python_launch_does() {
    let rig = Rig::new(|_| Box::pin(async {})).await;
    let url = rig.site.url("/");
    let seen = Arc::new(Mutex::new(None));
    let out = Arc::clone(&seen);
    let browser = LensBrowser::with_settings(rig.settings(step(move |ctx, _| {
        let (url, out) = (url.clone(), Arc::clone(&out));
        Box::pin(async move {
            ctx.page().goto(&url, 30_000).await?;
            let state: serde_json::Value = ctx
                .page()
                .evaluate(
                    "probe",
                    "({webdriver: navigator.webdriver, language: navigator.language, languages: navigator.languages, \
                      width: innerWidth, height: innerHeight, dpr: devicePixelRatio, touch: navigator.maxTouchPoints, \
                      focus: document.hasFocus()})",
                )
                .await?;
            *out.lock().unwrap() = Some(state);
            Ok(payload("ok"))
        })
    })));
    fetch(&browser).await.unwrap();
    let launched = chrome_command_lines(&rig.profile());
    browser.shutdown();
    let state = seen.lock().unwrap().clone().unwrap();
    assert_eq!(state["webdriver"], json!(false), "{state}");
    assert_eq!(state["language"], json!("en-US"));
    assert_eq!(
        (state["width"].clone(), state["height"].clone()),
        (json!(1280), json!(900))
    );
    assert_eq!(state["touch"], json!(0));
    assert_eq!(launched.len(), 1, "{launched:?}");
    let args = &launched[0];
    assert!(args.contains(&"--class=hatate-lens".to_owned()));
    assert!(!args.iter().any(|a| a.contains("enable-automation")));
    assert!(!args.iter().any(|a| a.starts_with("--headless")));
    assert!(rig.profile().is_dir());
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn the_warm_up_turns_safesearch_off_once_and_runs_both_searches() {
    let rig = Rig::new(|_| Box::pin(async {})).await;
    let browser = LensBrowser::with_settings(rig.settings(step(|ctx, _| {
        Box::pin(async move {
            ctx.warm_up().await?;
            Ok(payload("ok"))
        })
    })));
    fetch(&browser).await.unwrap();
    fetch(&browser).await.unwrap();
    browser.shutdown();
    let log = rig.site.log();
    // The click's own request may leave the stalled page after the next navigation starts, so
    // the three navigations are compared in order and the click by itself.
    let navigations: Vec<&String> = log.iter().filter(|l| !l.contains("/_click")).collect();
    assert_eq!(
        navigations,
        [
            "GET /safesearch",
            "GET /search?q=weather&hl=en",
            "GET /search?q=news&hl=en"
        ],
        "once per browser, not once per image"
    );
    let clicks: Vec<&String> = log.iter().filter(|l| l.contains("/_click")).collect();
    assert_eq!(clicks.len(), 1, "{log:?}");
    assert!(
        clicks[0].starts_with("GET /_click?id=off&x=118&y=130"),
        "{log:?}"
    );
    // 3000 after the SafeSearch page loads, 3000 after the click, 2000 after each warm-up page.
    assert_eq!(*rig.waits.waits.lock().unwrap(), [3.0, 3.0, 2.0, 2.0]);
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_robot_check_that_clears_within_the_wait_is_waited_out_and_the_user_is_called() {
    let slot: Arc<Mutex<Option<hatate_core::lens_browser::page::LensPage>>> = Arc::default();
    let hooked = Arc::clone(&slot);
    let rig = Rig::new(move |n| {
        let page = hooked.lock().unwrap().clone();
        Box::pin(async move {
            // The user answers the check on the 3rd poll: the frame goes.
            if n == 3
                && let Some(page) = page
            {
                let _: bool = page
                    .evaluate(
                        "answer",
                        "(document.querySelector('iframe').remove(), true)",
                    )
                    .await
                    .unwrap();
            }
        })
    })
    .await;
    rig.site.set_challenged(true);
    let url = rig.site.url("/search?q=x");
    let browser = LensBrowser::with_settings({
        let mut settings = rig.settings(step(move |ctx, _| {
            let (url, slot) = (url.clone(), Arc::clone(&slot));
            Box::pin(async move {
                ctx.page().goto(&url, 30_000).await?;
                *slot.lock().unwrap() = Some(ctx.page().clone());
                ctx.wait_out_challenge("after the upload").await?;
                Ok(payload("answered"))
            })
        }));
        settings.challenge_wait_ms = 10_000;
        settings
    });
    let got = fetch(&browser).await.unwrap();
    browser.shutdown();
    assert_eq!(got.payloads, ["answered"]);
    assert_eq!(*rig.waits.waits.lock().unwrap(), [1.0, 1.0, 1.0]);
    assert_eq!(
        *rig.attention.0.lock().unwrap(),
        ["ask hatate-lens", "quiet hatate-lens"]
    );
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_robot_check_that_is_not_answered_ends_the_search_unanswered() {
    let rig = Rig::new(|_| Box::pin(async {})).await;
    rig.site.set_challenged(true);
    let url = rig.site.url("/search?q=x");
    let browser = LensBrowser::with_settings({
        let mut settings = rig.settings(step(move |ctx, _| {
            let url = url.clone();
            Box::pin(async move {
                ctx.page().goto(&url, 30_000).await?;
                ctx.wait_out_challenge("after the upload").await?;
                Ok(payload("never"))
            })
        }));
        settings.challenge_wait_ms = 3_000;
        settings
    });
    let err = fetch(&browser).await.unwrap_err();
    browser.shutdown();
    assert_eq!(
        err,
        LensFetchError::ChallengeUnanswered(
            "the robot check in the browser window went unanswered".to_owned()
        )
    );
    assert_eq!(rig.waits.waits.lock().unwrap().len(), 3);
    assert_eq!(
        rig.attention.0.lock().unwrap().last().unwrap(),
        "quiet hatate-lens"
    );
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_window_closed_under_a_search_is_reopened_and_the_search_run_again_once() {
    let rig = Rig::new(|_| Box::pin(async {})).await;
    let profile = rig.profile();
    let url = rig.site.url("/");
    let runs = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&runs);
    let browser = LensBrowser::with_settings(rig.settings(step(move |ctx, _| {
        let (profile, url, counted) = (profile.clone(), url.clone(), Arc::clone(&counted));
        Box::pin(async move {
            let n = counted.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                // The user closes the window (here: the browser is killed) during the search.
                for args in chrome_command_lines(&profile) {
                    let _ = args;
                }
                kill_browser(&profile);
                std::thread::sleep(std::time::Duration::from_millis(500));
                ctx.page().goto(&url, 30_000).await?;
            }
            Ok(payload("second window"))
        })
    })));
    // First job: dies once, is retried in a new window, and the second run of the step is also
    // told the window is gone, so it ends as an error (a job is retried once).
    let err = fetch(&browser).await.unwrap_err();
    assert_eq!(
        err,
        LensFetchError::Browser(
            "Page.goto: Target page, context or browser has been closed".to_owned()
        )
    );
    assert_eq!(runs.load(Ordering::SeqCst), 2);
    // The next job gets a working browser.
    let ok = fetch(&browser).await.unwrap();
    assert_eq!(ok.payloads, ["second window"]);
    browser.shutdown();
}

fn kill_browser(profile: &Path) {
    let needle = format!("--user-data-dir={}", profile.display());
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let text = String::from_utf8_lossy(&raw).into_owned();
        if text.contains(&needle) && !text.contains("--type=") {
            let pid: i32 = entry.file_name().to_string_lossy().parse().unwrap();
            // SAFETY: SIGKILL to a process this test started.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_launch_that_fails_fails_every_waiting_job_and_names_a_missing_browser() {
    let rig = Rig::new(|_| Box::pin(async {})).await;
    // A "browser" that is not one.
    let bin = rig.dir.path().join("bin/chromium");
    std::fs::remove_file(&bin).unwrap();
    std::fs::write(&bin, "#!/bin/sh\necho 'boom'\nexit 3\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let browser =
        LensBrowser::with_settings(rig.settings(step(|_, _| Box::pin(async { Ok(payload("x")) }))));
    let (a, b) = tokio::join!(fetch(&browser), fetch(&browser));
    for err in [a.unwrap_err(), b.unwrap_err()] {
        let LensFetchError::Unavailable { message, missing } = err else {
            panic!("{err:?}")
        };
        assert_eq!(missing, None);
        assert!(
            message.starts_with("The browser could not be started: Browser closed unexpectedly"),
            "{message}"
        );
        assert!(message.contains("boom"));
    }
    browser.shutdown();
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn no_browser_at_all_is_the_missing_chromium_answer_before_any_launch() {
    let rig = Rig::new(|_| Box::pin(async {})).await;
    std::fs::remove_file(rig.dir.path().join("bin/chromium")).unwrap();
    let browser =
        LensBrowser::with_settings(rig.settings(step(|_, _| Box::pin(async { Ok(payload("x")) }))));
    let err = fetch(&browser).await.unwrap_err();
    let LensFetchError::Unavailable { missing, message } = err else {
        panic!()
    };
    assert_eq!(missing, Some(LensMissing::Chromium));
    assert!(!message.contains("pip"));
    assert!(!rig.profile().exists(), "nothing was started");
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_profile_another_chromium_holds_is_named_in_the_error() {
    let rig = Rig::new(|_| Box::pin(async {})).await;
    let held = LensBrowser::with_settings(rig.settings(step(|_, _| {
        Box::pin(async {
            tokio::time::sleep(std::time::Duration::from_secs(8)).await;
            Ok(payload("x"))
        })
    })));
    let holder = held.clone();
    let first = tokio::spawn(async move { fetch(&holder).await });
    // Wait for the first browser to hold the profile.
    for _ in 0..100 {
        // The profile is held once Chromium has made its lock.
        if std::fs::symlink_metadata(rig.profile().join("SingletonLock")).is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let second =
        LensBrowser::with_settings(rig.settings(step(|_, _| Box::pin(async { Ok(payload("y")) }))));
    let err = fetch(&second).await.unwrap_err();
    let LensFetchError::Unavailable { message, .. } = err else {
        panic!()
    };
    assert!(
        message.starts_with("The browser could not be started: Another Chromium is already using the Lens profile at "),
        "{message}"
    );
    assert!(message.ends_with("search again."));
    second.shutdown();
    held.shutdown();
    let _ = first.await;
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_job_ahead_of_shutdown_still_runs_and_a_page_error_is_its_first_line() {
    let rig = Rig::new(|_| Box::pin(async {})).await;
    let ran = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&ran);
    let browser = LensBrowser::with_settings(rig.settings(step(move |ctx, _| {
        let counted = Arc::clone(&counted);
        Box::pin(async move {
            counted.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            // A Playwright-style multi-line message: only the first line reaches the user.
            let _ = ctx;
            Err(StepError::Page(PageError::other(
                "Page.goto",
                "net::ERR_X\nCall log:\n  - navigating",
            )))
        })
    })));
    let queued = browser.clone();
    let job = tokio::spawn(async move { fetch(&queued).await });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let stopper = browser.clone();
    tokio::task::spawn_blocking(move || stopper.shutdown())
        .await
        .unwrap();
    let err = job.await.unwrap().unwrap_err();
    assert_eq!(
        err,
        LensFetchError::Browser("Page.goto: net::ERR_X".to_owned())
    );
    assert_eq!(ran.load(Ordering::SeqCst), 1);
}
