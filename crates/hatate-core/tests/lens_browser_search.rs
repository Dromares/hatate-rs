//! The per-search half of the Lens browser over a real Chromium (DAN-2165): a whole-image search,
//! a crop that is dragged back out, the explicit-results notice, the two result tabs, the tiles, a
//! rule34 search tile, empty results, "can't read file", a robot check, a window closed mid-search
//! and a timeout, against a fake results site on 127.0.0.1 (`common/lens_results.rs`; nothing here
//! reaches Google).
//!
//! These start a browser, so they are `#[ignore]`d and run with `--include-ignored`, as
//! `lens_browser_cdp.rs` does (`$HATATE_TEST_CHROMIUM`, `$HATATE_TEST_BROWSER_ARGS`). The browser
//! differential against Python's Playwright (`worktrees/DAN-2165-differential.log`) is the broad
//! check; these are the named ones. The Python test each one maps to is in `docs/lens-search.md`.

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::TestServer;
use common::lens_results::{
    ASYNC_URLS, Cfg, EXACT_URLS, MAIN_URLS, VISUAL_URLS, WIDEN_URLS, results_site,
};
use hatate_core::clock::{BoxFuture, Clock};
use hatate_core::engines::lens_search::{LensFetchError, LensPayloads};
use hatate_core::lens_browser::deps::BrowserEnv;
use hatate_core::lens_browser::page::LensPage;
use hatate_core::lens_browser::search::LensSearch;
use hatate_core::lens_browser::{
    JobRequest, LensBrowser, LensSettings, SearchStep, StepCtx, StepError, WindowAttention,
};

/// Every wait asked for, and a hook run on each: the test's way of moving the site on.
struct HookClock {
    waits: Mutex<Vec<f64>>,
    hook: Box<dyn Fn(usize, f64) -> BoxFuture<'static, ()> + Send + Sync>,
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
        let hook = (self.hook)(n, seconds);
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

/// Hands the page to the hook, then runs the real step.
struct Recording {
    inner: LensSearch,
    page: Arc<Mutex<Option<LensPage>>>,
}

impl SearchStep for Recording {
    fn one(
        &self,
        ctx: StepCtx,
        job: Arc<JobRequest>,
    ) -> BoxFuture<'_, Result<LensPayloads, StepError>> {
        *self.page.lock().unwrap() = Some(ctx.page().clone());
        SearchStep::one(&self.inner, ctx, job)
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
    cfg: Arc<Mutex<Cfg>>,
    server: TestServer,
    waits: Arc<HookClock>,
    attention: Arc<Calls>,
    page: Arc<Mutex<Option<LensPage>>>,
}

type Hook = Box<dyn Fn(usize, f64, Option<LensPage>) -> BoxFuture<'static, ()> + Send + Sync>;

impl Rig {
    async fn new(cfg: Cfg, hook: Hook) -> Rig {
        let dir = tempfile::Builder::new()
            .prefix("hatate-dan2165-")
            .tempdir()
            .unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(test_browser(), bin.join("chromium")).unwrap();
        let cfg = Arc::new(Mutex::new(cfg));
        let server = TestServer::http(results_site(Arc::clone(&cfg))).await;
        let page: Arc<Mutex<Option<LensPage>>> = Arc::default();
        let slot = Arc::clone(&page);
        Rig {
            cfg,
            server,
            waits: Arc::new(HookClock {
                waits: Mutex::new(Vec::new()),
                hook: Box::new(move |n, seconds| hook(n, seconds, slot.lock().unwrap().clone())),
            }),
            attention: Arc::new(Calls::default()),
            page,
            dir,
        }
    }

    fn profile(&self) -> PathBuf {
        self.dir
            .path()
            .join("home/.config/hatate-linux/lens_profile")
    }

    fn browser(&self) -> LensBrowser {
        let mut settings = LensSettings::shipped(self.profile());
        settings.env = BrowserEnv {
            home: Some(self.dir.path().join("home")),
            path: Some(OsString::from(self.dir.path().join("bin"))),
            ..BrowserEnv::default()
        };
        settings.safesearch_url = self.server.url("/safesearch");
        settings.warmup_searches = vec![
            self.server.url("/warm?q=weather"),
            self.server.url("/warm?q=news"),
        ];
        settings.waits = Arc::clone(&self.waits) as Arc<dyn Clock>;
        settings.attention = Arc::clone(&self.attention) as Arc<dyn WindowAttention>;
        settings.step = Arc::new(Recording {
            inner: LensSearch::with_urls(&self.server.url("/"), &self.server.url("/upload")),
            page: Arc::clone(&self.page),
        });
        settings.extra_launch_args = std::env::var("HATATE_TEST_BROWSER_ARGS")
            .map(|v| v.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default();
        LensBrowser::with_settings(settings)
    }

    /// The paths the site was asked for, in order, without the page's own beacons.
    fn requests(&self) -> Vec<String> {
        self.server
            .recorded()
            .iter()
            .map(|r| format!("{} {}", r.method, r.target))
            .collect()
    }

    fn asked(&self) -> Vec<String> {
        self.attention.0.lock().unwrap().clone()
    }
}

fn plain() -> Hook {
    Box::new(|_, _, _| Box::pin(async {}))
}

async fn search(browser: &LensBrowser, whole: bool) -> Result<LensPayloads, LensFetchError> {
    browser
        .fetch(b"\xff\xd8\xff\xe0 image", "a.jpg", 6.0, whole)
        .await
}

fn has(payloads: &[String], urls: &[&str]) -> bool {
    urls.iter().all(|u| payloads.iter().any(|p| p.contains(u)))
}

fn kill_browser(profile: &Path) {
    let needle = format!("--user-data-dir={}", profile.display());
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<String> = raw
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        if args.contains(&needle)
            && !args.iter().any(|a| a.starts_with("--type="))
            && let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>()
        {
            // SAFETY: SIGKILL to the browser this test started.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
}

/// The verifier's PASS names this test: the whole search through the binary's own constructor
/// (`LensSearch::with_urls` is the only way to point the page elsewhere), the upload as the page
/// posts it, both tabs, and the tiles.
#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_whole_image_search_uploads_reads_both_tabs_and_collects_the_tiles() {
    let rig = Rig::new(
        Cfg {
            tiles: vec![
                ("Rule 34 / parent:10755759 250x177 Rule 34", true),
                ("x", true),
                ("Other post 1x1", false),
            ],
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = rig.browser();
    let found = search(&browser, true).await.unwrap();
    browser.shutdown();
    assert!(has(&found.payloads, &MAIN_URLS[..3]) && has(&found.payloads, &ASYNC_URLS));
    assert!(has(&found.payloads, &VISUAL_URLS) && has(&found.payloads, &EXACT_URLS));
    assert!(!has(&found.payloads, &WIDEN_URLS), "nothing was widened");
    // Only the tile with a thumbnail and some text of its own is read out of the page.
    let tiles = found.tiles.as_array().unwrap();
    assert_eq!(tiles.len(), 2, "{tiles:?}");
    assert_eq!(
        tiles[0]["text"],
        "Rule 34 / parent:10755759 250x177 Rule 34"
    );
    assert!(
        tiles[0]["thumb"]
            .as_str()
            .unwrap()
            .starts_with("data:image/jpeg;base64,")
    );
    let requests = rig.requests();
    let upload = requests.iter().position(|r| r == "POST /upload").unwrap();
    assert_eq!(
        requests[upload - 1],
        "GET /",
        "the page is opened before the form is posted"
    );
    let nav: Vec<&String> = requests
        .iter()
        .filter(|r| r.contains("&tab=") && r.starts_with("GET /search?ep"))
        .collect();
    assert_eq!(
        nav,
        [
            "GET /search?ep=1&vsrid=abc123&tab=main",
            "GET /search?ep=1&vsrid=abc123&tab=visual",
            "GET /search?ep=1&vsrid=abc123&tab=exact"
        ]
    );
    let upload = rig
        .server
        .recorded()
        .into_iter()
        .find(|r| r.method == "POST")
        .unwrap();
    assert!(
        upload
            .header("content-type")
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    let body = String::from_utf8_lossy(&upload.body).into_owned();
    assert!(
        body.contains("name=\"encoded_image\"; filename=\"a.jpg\""),
        "{body}"
    );
    assert!(
        body.contains("Content-Type: image/jpeg"),
        "the real media type, not octet-stream"
    );
    assert_eq!(
        rig.asked(),
        ["quiet hatate-lens", "quiet hatate-lens"],
        "an ordinary search never asks"
    );
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_partial_search_area_is_dragged_out_and_the_widened_results_win() {
    let cfg = Cfg {
        crop: [9, 4, 87, 90],
        ..Cfg::default()
    };
    let rig = Rig::new(cfg.clone(), plain()).await;
    let browser = rig.browser();
    let widened = search(&browser, true).await.unwrap();
    // The first drag also posts the crop to the page, which answers with the widened results.
    assert!(
        has(&widened.payloads, &WIDEN_URLS),
        "{:?}",
        widened.payloads.len()
    );
    assert!(
        !has(&widened.payloads, &MAIN_URLS[..1]),
        "the cropped page's own results are left behind"
    );
    assert!(
        rig.requests()
            .iter()
            .any(|r| r.contains("widen=1&crop=0,0,100,100")),
        "{:?}",
        rig.requests()
    );
    // With the search area left alone the cropped results are what there is.
    *rig.cfg.lock().unwrap() = cfg;
    let alone = search(&browser, false).await.unwrap();
    browser.shutdown();
    assert!(has(&alone.payloads, &MAIN_URLS[..1]) && !has(&alone.payloads, &WIDEN_URLS));
    let widens = rig
        .requests()
        .iter()
        .filter(|r| r.contains("widen=1"))
        .count();
    assert_eq!(widens, 1, "whole_image=false never drags");
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_widen_that_brings_nothing_new_falls_back_to_the_cropped_results() {
    let rig = Rig::new(
        Cfg {
            crop: [9, 4, 87, 90],
            widen_hint: false,
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = rig.browser();
    let found = search(&browser, true).await.unwrap();
    browser.shutdown();
    // The tab pages after the drag still come back, so the list is not empty; the point is that
    // the widened response (which names only Google) was not captured and nothing is lost.
    assert!(!found.payloads.is_empty());
    assert!(has(&found.payloads, &VISUAL_URLS));
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn the_explicit_results_notice_is_confirmed_when_there_is_no_tab_to_open() {
    let rig = Rig::new(
        Cfg {
            tabs: "none",
            explicit: "notice",
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = rig.browser();
    let found = search(&browser, true).await.unwrap();
    browser.shutdown();
    assert!(
        has(&found.payloads, &EXACT_URLS),
        "the notice leads to Exact matches"
    );
    assert!(rig.requests().iter().any(|r| r.contains("tab=exact")));
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn controls_that_are_not_there_cost_no_click_timeout() {
    let rig = Rig::new(
        Cfg {
            tabs: "none",
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = rig.browser();
    let started = std::time::Instant::now();
    search(&browser, true).await.unwrap();
    browser.shutdown();
    // Python measured 8 s wasted per absent tab: nothing waits on a control that is not there.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "{:?}",
        started.elapsed()
    );
    let waits = rig.waits.waits.lock().unwrap().clone();
    assert!(waits.iter().all(|w| *w <= 3.0), "{waits:?}");
    assert!(waits.iter().sum::<f64>() < 60.0, "{waits:?}");
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn empty_results_come_back_as_no_payloads_and_no_tiles() {
    let rig = Rig::new(
        Cfg {
            doc_hint: false,
            async_hint: false,
            widen_hint: false,
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = rig.browser();
    let found = search(&browser, true).await.unwrap();
    browser.shutdown();
    assert!(found.payloads.is_empty(), "{:?}", found.payloads.len());
    assert_eq!(found.tiles, serde_json::json!([]));
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_page_that_cannot_read_the_file_ends_in_the_navigation_timeout() {
    let rig = Rig::new(
        Cfg {
            upload: "unreadable",
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = rig.browser();
    let err = browser
        .fetch(b"\xff\xd8\xff\xe0 image", "a.jpg", 2.0, true)
        .await
        .unwrap_err();
    browser.shutdown();
    assert_eq!(
        err,
        LensFetchError::Browser("Timeout 2000ms exceeded.".to_owned())
    );
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn an_upload_that_never_answers_times_out_inside_the_job_budget() {
    let rig = Rig::new(
        Cfg {
            upload: "hang",
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = rig.browser();
    let started = std::time::Instant::now();
    let err = browser
        .fetch(b"\xff\xd8\xff\xe0 image", "a.jpg", 2.0, true)
        .await
        .unwrap_err();
    let took = started.elapsed();
    browser.shutdown();
    assert!(matches!(err, LensFetchError::Browser(_)), "{err:?}");
    assert!(took < std::time::Duration::from_secs(25), "{took:?}");
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_robot_check_after_the_upload_calls_the_user_and_carries_on_once_cleared() {
    let rig = Rig::new(
        Cfg {
            upload: "sorry",
            ..Cfg::default()
        },
        Box::new(|n, seconds, page| {
            Box::pin(async move {
                // The user answers on the second poll of the check (the polls are 1000 ms waits).
                if seconds == 1.0
                    && n >= 1
                    && let Some(page) = page
                    && page.url().await.contains("/sorry/")
                {
                    page.goto("about:blank", 5_000).await.ok();
                }
            })
        }),
    )
    .await;
    let browser = rig.browser();
    let _ = search(&browser, true).await;
    browser.shutdown();
    let asked = rig.asked();
    assert!(asked.contains(&"ask hatate-lens".to_owned()), "{asked:?}");
    let ask = asked.iter().position(|a| a.starts_with("ask")).unwrap();
    assert_eq!(
        asked[ask + 1],
        "quiet hatate-lens",
        "the window goes quiet again"
    );
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_robot_check_that_goes_unanswered_is_reported_and_the_window_goes_quiet() {
    let rig = Rig::new(
        Cfg {
            upload: "sorry",
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = {
        // A short wait: three 1000 ms polls.
        let mut settings = LensSettings::shipped(rig.profile());
        settings.env = BrowserEnv {
            home: Some(rig.dir.path().join("home")),
            path: Some(OsString::from(rig.dir.path().join("bin"))),
            ..BrowserEnv::default()
        };
        settings.safesearch_url = rig.server.url("/safesearch");
        settings.warmup_searches = vec![rig.server.url("/warm?q=weather")];
        settings.waits = Arc::clone(&rig.waits) as Arc<dyn Clock>;
        settings.attention = Arc::clone(&rig.attention) as Arc<dyn WindowAttention>;
        settings.step = Arc::new(LensSearch::with_urls(
            &rig.server.url("/"),
            &rig.server.url("/upload"),
        ));
        settings.challenge_wait_ms = 3000;
        settings.extra_launch_args = std::env::var("HATATE_TEST_BROWSER_ARGS")
            .map(|v| v.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default();
        LensBrowser::with_settings(settings)
    };
    let err = search(&browser, true).await.unwrap_err();
    browser.shutdown();
    assert_eq!(
        err,
        LensFetchError::ChallengeUnanswered(
            "the robot check in the browser window went unanswered".to_owned()
        )
    );
    let asked = rig.asked();
    assert!(asked.iter().any(|a| a == "ask hatate-lens"), "{asked:?}");
    assert_eq!(asked.last().unwrap(), "quiet hatate-lens");
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_check_after_opening_a_tab_asks_for_the_user_too() {
    let rig = Rig::new(
        Cfg {
            challenge_on_tabs: true,
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = rig.browser();
    // Unanswered: the check on the Visual matches tab is noticed after the click, not after the
    // upload, and ends the search with the user called.
    let result = tokio::time::timeout(std::time::Duration::from_secs(40), async {
        let page = Arc::clone(&rig.page);
        let task = tokio::spawn({
            let browser = browser.clone();
            async move { search(&browser, true).await }
        });
        // Answer the check by navigating away once the user has been called.
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            if rig.asked().iter().any(|a| a.starts_with("ask")) {
                let current = page.lock().unwrap().clone();
                if let Some(current) = current {
                    current.goto("about:blank", 5_000).await.ok();
                }
                break;
            }
        }
        task.await.unwrap()
    })
    .await;
    browser.shutdown();
    assert!(
        result.is_ok(),
        "the search ended once the check was answered"
    );
    assert!(
        rig.asked().iter().any(|a| a == "ask hatate-lens"),
        "{:?}",
        rig.asked()
    );
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_window_closed_mid_search_is_reopened_and_the_search_runs_again() {
    let profile: Arc<Mutex<Option<PathBuf>>> = Arc::default();
    let killer = Arc::clone(&profile);
    let rig = Rig::new(
        Cfg {
            crop: [9, 4, 87, 90],
            ..Cfg::default()
        },
        Box::new(move |n, seconds, _| {
            let killer = Arc::clone(&killer);
            Box::pin(async move {
                // A settle poll of the first search.
                let target = killer.lock().unwrap().clone();
                if n == 8
                    && seconds == 0.25
                    && let Some(profile) = target
                {
                    kill_browser(&profile);
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                }
            })
        }),
    )
    .await;
    *profile.lock().unwrap() = Some(rig.profile());
    let browser = rig.browser();
    let found = search(&browser, true).await.unwrap();
    browser.shutdown();
    assert!(
        has(&found.payloads, &VISUAL_URLS),
        "the second window finished the search"
    );
    let uploads = rig
        .requests()
        .iter()
        .filter(|r| *r == "POST /upload")
        .count();
    assert_eq!(uploads, 2, "the interrupted search ran again from the top");
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_slow_response_is_still_waited_for() {
    let rig = Rig::new(
        Cfg {
            d_async_ms: 900,
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    let browser = rig.browser();
    let found = search(&browser, true).await.unwrap();
    browser.shutdown();
    assert!(
        has(&found.payloads, &ASYNC_URLS),
        "{:?} {:?}",
        found.payloads.iter().map(String::len).collect::<Vec<_>>(),
        rig.requests()
    );
}

#[tokio::test]
#[ignore = "starts a real Chromium"]
async fn a_step_whose_response_never_comes_waits_no_longer_than_the_fixed_wait_it_replaces() {
    let rig = Rig::new(
        Cfg {
            tabs: "none",
            ..Cfg::default()
        },
        plain(),
    )
    .await;
    // Every wait a step asks for is at most its cap (6000 ms for the first, 250 ms polls, 1200
    // after a drag, 500 for the tiles).
    let browser = rig.browser();
    search(&browser, true).await.unwrap();
    browser.shutdown();
    let waits = rig.waits.waits.lock().unwrap().clone();
    let polls = waits.iter().filter(|w| **w == 0.25).count();
    assert!(
        polls <= 6000 / 250,
        "{polls} polls for the first settle alone is too many: {waits:?}"
    );
}
