//! Starting the Chromium (`LensBrowser._launch_context`): the argv, the process, and the DevTools
//! endpoint it announces.
//!
//! **Why not `chromiumoxide`'s own launcher.** It adds `--enable-automation` (which sets
//! `navigator.webdriver`, the thing Python's `ignore_default_args=["--enable-automation"]` exists to
//! undo), a puppeteer flag set that differs from Playwright's, `--disable-extensions` and
//! `--no-sandbox` in its own places. Lens keeps Python's page-observable state, so the argv here is
//! the one Playwright 1.63 builds for `launch_persistent_context(... ignore_default_args=
//! ["--enable-automation"], args=[--class, --disable-blink-features], headless=False)`, with one
//! difference that cannot be matched: Playwright talks to the browser over a pipe
//! (`--remote-debugging-pipe`), `chromiumoxide` over a WebSocket, so the Rust argv has
//! `--remote-debugging-port=0` in its place (`docs/lens-browser.md`, class A).

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::deps::LaunchFailure;

/// `WINDOW_CLASS`: the Lens window's own identity (Wayland app id and X11 `WM_CLASS`).
pub const WINDOW_CLASS: &str = "hatate-lens";

/// Playwright's default launch timeout.
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Playwright 1.63's default Chromium flags, in its order, without `--enable-automation`.
const PLAYWRIGHT_DEFAULT_ARGS: [&str; 38] = [
    "--disable-field-trial-config",
    "--disable-background-networking",
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-back-forward-cache",
    "--disable-breakpad",
    "--disable-client-side-phishing-detection",
    "--disable-component-extensions-with-background-pages",
    "--disable-component-update",
    "--no-default-browser-check",
    "--disable-default-apps",
    "--disable-dev-shm-usage",
    "--disable-edgeupdater",
    "--disable-extensions",
    "--disable-features=AvoidUnnecessaryBeforeUnloadCheckSync,DestroyProfileOnBrowserClose,DialMediaRouteProvider,GlobalMediaControls,HttpsUpgrades,LensOverlay,MediaRouter,PaintHolding,ThirdPartyStoragePartitioning,BlockOriginHeaderModificationOnRedirect,Translate,AutoDeElevate,OptimizationHints,msForceBrowserSignIn,msEdgeUpdateLaunchServicesPreferredVersion",
    "--enable-features=CDPScreenshotNewSurface",
    "--allow-pre-commit-input",
    "--disable-hang-monitor",
    "--disable-ipc-flooding-protection",
    "--disable-popup-blocking",
    "--disable-prompt-on-repost",
    "--disable-renderer-backgrounding",
    "--disable-updater-scheduler",
    "--force-color-profile=srgb",
    "--metrics-recording-only",
    "--no-first-run",
    "--password-store=basic",
    "--use-mock-keychain",
    "--no-service-autorun",
    "--export-tagged-pdf",
    "--disable-search-engine-choice-screen",
    "--unsafely-disable-devtools-self-xss-warnings",
    "--edge-skip-compat-layer-relaunch",
    "--disable-infobars",
    "--disable-search-engine-choice-screen",
    "--disable-sync",
    "--enable-unsafe-swiftshader",
    // `chromiumSandbox` is off by default in Playwright.
    "--no-sandbox",
];

/// What the launch is given.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub executable: PathBuf,
    /// `PROFILE_DIR`.
    pub profile_dir: PathBuf,
    /// Flags added after Lens's own, for a test that has to run without a window server
    /// (`--ozone-platform=headless`). The shipped constructor leaves this empty.
    pub extra_args: Vec<String>,
}

/// The argv after the executable. Headed, `--class=hatate-lens`, no `--enable-automation`,
/// automation-controlled blink feature off, the profile, the DevTools port, `about:blank`.
pub fn argv(spec: &LaunchSpec) -> Vec<String> {
    let mut args: Vec<String> = PLAYWRIGHT_DEFAULT_ARGS.iter().map(|a| (*a).to_owned()).collect();
    args.push(format!("--class={WINDOW_CLASS}"));
    args.push("--disable-blink-features=AutomationControlled".to_owned());
    args.extend(spec.extra_args.iter().cloned());
    args.push(format!("--user-data-dir={}", spec.profile_dir.display()));
    args.push("--remote-debugging-port=0".to_owned());
    args.push("about:blank".to_owned());
    args
}

/// A running Chromium. Dropping it kills the process, so a Hatate that is stopped by an error
/// does not leave a browser behind.
pub struct RunningChrome {
    child: Child,
    ws_url: String,
}

impl RunningChrome {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// `ws://127.0.0.1:<port>/devtools/browser/<id>`.
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Whether the process is still running.
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Waits up to `grace` for the process to exit by itself (after `Browser.close`), then kills it.
    pub fn finish(&mut self, grace: Duration) {
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if !self.is_running() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.kill();
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for RunningChrome {
    fn drop(&mut self) {
        if self.is_running() {
            self.kill();
        }
    }
}

/// Reads one pipe line by line into the channel, then ends.
fn pump(source: impl Read + Send + 'static, lines: mpsc::Sender<String>) {
    std::thread::spawn(move || {
        for line in BufReader::new(source).lines() {
            let Ok(line) = line else { break };
            if lines.send(line).is_err() {
                // Nobody is reading any more: keep draining so the browser never blocks on a full pipe.
                continue;
            }
        }
    });
}

fn io_failure(err: &std::io::Error) -> LaunchFailure {
    LaunchFailure::Spawn {
        kind: err.kind(),
        text: err.to_string(),
    }
}

/// Starts the browser and waits for it to announce its DevTools endpoint.
///
/// Blocking: a caller on an async runtime runs it on a blocking thread.
pub fn launch(spec: &LaunchSpec, timeout: Duration) -> Result<RunningChrome, LaunchFailure> {
    let mut command = Command::new(&spec.executable);
    command
        .args(argv(spec))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    set_parent_death_signal(&mut command);
    let mut child = command.spawn().map_err(|err| io_failure(&err))?;

    let (tx, rx) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        pump(out, tx.clone());
    }
    if let Some(err) = child.stderr.take() {
        pump(err, tx);
    }

    let deadline = Instant::now() + timeout;
    let mut log: Vec<String> = Vec::new();
    let mut chrome = RunningChrome {
        child,
        ws_url: String::new(),
    };
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(line) => {
                if let Some(url) = line.trim().strip_prefix("DevTools listening on ") {
                    chrome.ws_url = url.trim().to_owned();
                    // The pumps keep draining after this receiver is dropped.
                    return Ok(chrome);
                }
                log.push(line);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                chrome.kill();
                return Err(LaunchFailure::NoEndpoint {
                    text: browser_log("Timeout waiting for the browser to start", &log),
                });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = chrome.child.wait().ok();
                return Err(LaunchFailure::Exited {
                    text: browser_log(
                        &format!(
                            "Browser closed unexpectedly (exit status {})",
                            status.map_or_else(|| "unknown".to_owned(), |s| s.to_string())
                        ),
                        &log,
                    ),
                });
            }
        }
    }
}

fn browser_log(headline: &str, log: &[String]) -> String {
    let tail: Vec<&str> = log
        .iter()
        .rev()
        .take(40)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{headline}\nBrowser logs:\n{}", tail.join("\n"))
}

/// The browser dies with Hatate: `PR_SET_PDEATHSIG` (a WebSocket, unlike Playwright's pipe, does
/// not close the browser when the parent is killed).
fn set_parent_death_signal(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: `prctl` is async-signal-safe and touches only the calling (child) process.
    unsafe {
        command.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
}

/// `PROFILE_DIR.mkdir(parents=True, exist_ok=True)`.
pub fn ensure_profile_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> LaunchSpec {
        LaunchSpec {
            executable: PathBuf::from("/x/chrome"),
            profile_dir: PathBuf::from("/p/lens_profile"),
            extra_args: Vec::new(),
        }
    }

    #[test]
    fn the_argv_keeps_the_page_observable_state_of_the_python_launch() {
        let args = argv(&spec());
        assert!(!args.iter().any(|a| a.starts_with("--enable-automation")));
        for needed in [
            "--class=hatate-lens",
            "--disable-blink-features=AutomationControlled",
            "--user-data-dir=/p/lens_profile",
            "--no-sandbox",
        ] {
            assert!(args.iter().any(|a| a == needed), "missing {needed}");
        }
        assert!(!args.iter().any(|a| a.starts_with("--headless")), "Lens is headed");
        assert_eq!(args.last().map(String::as_str), Some("about:blank"));
        // The DevTools transport is the one flag that differs from Playwright's launch.
        assert!(args.iter().any(|a| a == "--remote-debugging-port=0"));
        assert!(!args.iter().any(|a| a.contains("remote-debugging-pipe")));
    }

    #[test]
    fn a_test_only_flag_goes_before_the_profile_and_the_shipped_argv_has_none() {
        let mut with = spec();
        with.extra_args = vec!["--ozone-platform=headless".to_owned()];
        let args = argv(&with);
        let extra = args.iter().position(|a| a == "--ozone-platform=headless").unwrap();
        let profile = args.iter().position(|a| a.starts_with("--user-data-dir")).unwrap();
        assert!(extra < profile);
        assert!(!argv(&spec()).iter().any(|a| a.contains("ozone")));
    }

    #[test]
    fn a_missing_executable_fails_as_a_spawn_error_not_found() {
        let err = launch(&spec(), Duration::from_secs(1)).err().unwrap();
        assert!(matches!(
            err,
            LaunchFailure::Spawn {
                kind: std::io::ErrorKind::NotFound,
                ..
            }
        ));
    }

    #[test]
    fn a_browser_that_exits_before_announcing_is_an_exited_failure_with_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("chrome");
        std::fs::write(&exe, "#!/bin/sh\necho 'Opening in existing browser session.'\n").unwrap();
        std::fs::set_permissions(
            &exe,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let spec = LaunchSpec {
            executable: exe,
            ..spec()
        };
        match launch(&spec, Duration::from_secs(5)).err().unwrap() {
            LaunchFailure::Exited { text } => {
                assert!(text.contains("Opening in existing browser session."), "{text}")
            }
            other => panic!("{other:?}"),
        }
    }
}
