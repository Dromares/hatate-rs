//! The page, as Lens's steps drive it: the few Playwright calls `lens_browser.py`'s lifecycle makes
//! (`goto`, `wait_for_timeout`, `bring_to_front`, `url`, `locator(...).count()/nth()/inner_text()/
//! get_attribute()/click()`), over the CDP client.
//!
//! **Errors.** A call that fails because the window, page or browser is gone says what Playwright's
//! `TargetClosedError` says, behind the call's own name (`Page.goto: Target page, context or
//! browser has been closed`), because `_browser_gone` recognises it by those words and
//! `_first_line` shows the first line of it to the user. Every page call also fails that way once
//! the connection to the browser has closed, whatever the CDP client said.
//!
//! **Cut (5.b).** `click` scrolls the element into view, waits until it has a box, and presses at
//! the centre of it with `mouseMoved`, `mousePressed`, `mouseReleased`, which is what Playwright
//! sends. It does not wait for the element to stop moving or check what is under the pointer; the
//! steps 5.c ports need neither.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chromiumoxide::cdp::browser_protocol::browser::{
    Bounds, GetWindowForTargetParams, SetWindowBoundsParams,
};
use chromiumoxide::cdp::browser_protocol::emulation::{
    SetDeviceMetricsOverrideParams, SetFocusEmulationEnabledParams, SetLocaleOverrideParams,
    SetUserAgentOverrideParams,
};
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
};
use chromiumoxide::cdp::browser_protocol::page::{EventDomContentEventFired, NavigateParams};
use chromiumoxide::error::CdpError;
use futures::StreamExt;
use serde::de::DeserializeOwned;

use crate::clock::Clock;

/// The words Playwright's `TargetClosedError` carries; `_browser_gone` looks for `has been closed`.
pub const TARGET_CLOSED: &str = "Target page, context or browser has been closed";

/// `viewport={"width": 1280, "height": 900}`.
pub const VIEWPORT: (u32, u32) = (1280, 900);

/// `locale="en-US"`.
pub const LOCALE: &str = "en-US";

/// Playwright's default timeout for an action.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// What a failed page call says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageError {
    message: String,
    gone: bool,
}

impl PageError {
    /// `TargetClosedError`.
    pub fn target_closed(api: &str) -> Self {
        PageError {
            message: format!("{api}: {TARGET_CLOSED}"),
            gone: true,
        }
    }

    /// Any other exception a page call raises (`Error`, `TimeoutError`).
    pub fn other(api: &str, message: impl std::fmt::Display) -> Self {
        PageError {
            message: format!("{api}: {message}"),
            gone: false,
        }
    }

    /// `type(exc).__name__ == "TargetClosedError"`.
    pub fn is_target_closed(&self) -> bool {
        self.gone
    }

    /// `str(exc)`.
    pub fn message(&self) -> &str {
        &self.message
    }

    pub(super) fn from_cdp(api: &str, error: &CdpError) -> Self {
        let text = error.to_string();
        let gone = match error {
            CdpError::Ws(_)
            | CdpError::Io(_)
            | CdpError::ChannelSendError(_)
            | CdpError::NoResponse => true,
            CdpError::Chrome(_) | CdpError::ChromeMessage(_) => {
                text.contains("Target closed")
                    || text.contains("No target with given id")
                    || text.contains("Session with given id not found")
                    || text.contains("Inspected target navigated or closed")
                    || text.contains("Browser has been closed")
            }
            _ => false,
        };
        if gone {
            PageError::target_closed(api)
        } else {
            PageError::other(api, text)
        }
    }
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PageError {}

/// A Playwright `Page`.
#[derive(Clone)]
pub struct LensPage {
    pub(super) page: chromiumoxide::Page,
    /// Set when the connection to the browser has closed.
    pub(super) gone: Arc<AtomicBool>,
    /// `page.wait_for_timeout(ms)` sleeps on this, so a test sees every wait that was asked for.
    waits: Arc<dyn Clock>,
    /// `page.mouse`: where the pointer is and whether the left button is down.
    pub(super) mouse: Arc<std::sync::Mutex<super::elements::MouseState>>,
}

impl LensPage {
    pub(super) fn new(
        page: chromiumoxide::Page,
        gone: Arc<AtomicBool>,
        waits: Arc<dyn Clock>,
    ) -> Self {
        LensPage {
            page,
            gone,
            waits,
            mouse: Arc::default(),
        }
    }

    /// The CDP page, for the steps that need more than this wrapper offers.
    pub fn raw(&self) -> &chromiumoxide::Page {
        &self.page
    }

    pub(super) fn check(&self, api: &str) -> Result<(), PageError> {
        if self.gone.load(Ordering::SeqCst) {
            Err(PageError::target_closed(api))
        } else {
            Ok(())
        }
    }

    /// `page.url`: the address of the page, `""` when it cannot be read.
    pub async fn url(&self) -> String {
        match self.page.url().await {
            Ok(Some(url)) => url,
            _ => String::new(),
        }
    }

    /// Applies what `launch_persistent_context(locale="en-US", viewport=1280x900)` applies to a
    /// page: the metrics override, the locale and `Accept-Language`, and focus emulation.
    pub(super) async fn emulate(&self) -> Result<(), PageError> {
        let api = "BrowserType.launch_persistent_context";
        let cdp = |e: CdpError| PageError::from_cdp(api, &e);
        // A headed window is sized so the viewport fits in it, with the fixed insets Playwright
        // adds on Linux for a persistent context (8 wide; 85 for the browser's own chrome plus 46).
        let window = self
            .page
            .execute(GetWindowForTargetParams::default())
            .await
            .map_err(cdp)?
            .result
            .window_id;
        self.page
            .execute(SetWindowBoundsParams::new(
                window,
                Bounds::builder()
                    .width(i64::from(VIEWPORT.0) + 8)
                    .height(i64::from(VIEWPORT.1) + 85 + 46)
                    .build(),
            ))
            .await
            .map_err(cdp)?;
        self.page
            .execute(
                SetDeviceMetricsOverrideParams::builder()
                    .width(i64::from(VIEWPORT.0))
                    .height(i64::from(VIEWPORT.1))
                    // No `screen` option: the screen is the viewport.
                    .screen_width(i64::from(VIEWPORT.0))
                    .screen_height(i64::from(VIEWPORT.1))
                    .device_scale_factor(1.0)
                    .mobile(false)
                    .build()
                    .map_err(|e| PageError::other(api, e))?,
            )
            .await
            .map_err(cdp)?;
        self.page
            .execute(SetLocaleOverrideParams {
                locale: Some(LOCALE.to_owned()),
            })
            .await
            .map_err(cdp)?;
        self.page
            .execute(SetUserAgentOverrideParams {
                // Empty: the browser's own, with only `Accept-Language` changed.
                user_agent: String::new(),
                accept_language: Some(LOCALE.to_owned()),
                platform: None,
                user_agent_metadata: None,
            })
            .await
            .map_err(cdp)?;
        self.page
            .execute(SetFocusEmulationEnabledParams::new(true))
            .await
            .map_err(cdp)?;
        Ok(())
    }

    /// `page.goto(url, wait_until="domcontentloaded", timeout=timeout_ms)`.
    pub async fn goto(&self, url: &str, timeout_ms: u64) -> Result<(), PageError> {
        const API: &str = "Page.goto";
        self.check(API)?;
        let cdp = |e: CdpError| PageError::from_cdp(API, &e);
        let mut loaded = self
            .page
            .event_listener::<EventDomContentEventFired>()
            .await
            .map_err(cdp)?;
        let navigation = async {
            let started = self
                .page
                .execute(NavigateParams::new(url))
                .await
                .map_err(cdp)?;
            if let Some(text) = &started.result.error_text {
                return Err(PageError::other(API, format!("{text} at {url}")));
            }
            // Same-document navigations have no loader and fire no event.
            if started.result.loader_id.is_none() {
                return Ok(());
            }
            match loaded.next().await {
                Some(_) => Ok(()),
                None => Err(PageError::target_closed(API)),
            }
        };
        // One budget for the whole navigation, as Playwright's `timeout` is.
        match tokio::time::timeout(Duration::from_millis(timeout_ms), navigation).await {
            Ok(result) => result,
            Err(_) => Err(PageError::other(
                API,
                format!("Timeout {timeout_ms}ms exceeded."),
            )),
        }
    }

    /// `page.wait_for_timeout(ms)`.
    pub async fn wait_for_timeout(&self, ms: u64) -> Result<(), PageError> {
        self.check("Page.wait_for_timeout")?;
        self.waits.sleep(ms as f64 / 1000.0).await;
        self.check("Page.wait_for_timeout")
    }

    /// `page.bring_to_front()`.
    pub async fn bring_to_front(&self) -> Result<(), PageError> {
        self.check("Page.bring_to_front")?;
        self.page
            .bring_to_front()
            .await
            .map(|_| ())
            .map_err(|e| PageError::from_cdp("Page.bring_to_front", &e))
    }

    /// `page.evaluate(expression)` for an expression that yields JSON.
    pub async fn evaluate<T: DeserializeOwned>(
        &self,
        api: &str,
        expression: &str,
    ) -> Result<T, PageError> {
        self.check(api)?;
        let result = self
            .page
            .evaluate(expression)
            .await
            .map_err(|e| PageError::from_cdp(api, &e))?;
        result.into_value().map_err(|e| {
            PageError::other(
                api,
                format!("the page returned a value that cannot be read: {e}"),
            )
        })
    }

    /// `page.locator(selector)`.
    pub fn locator(&self, selector: &str) -> Locator {
        let quoted = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_owned());
        Locator {
            page: self.clone(),
            all: format!("{QUERY_ALL}({quoted})"),
            index: None,
        }
    }
}

/// A Playwright `Locator`: a CSS selector, optionally narrowed to one match.
#[derive(Clone)]
pub struct Locator {
    pub(super) page: LensPage,
    /// A JavaScript expression that yields every match as an array.
    pub(super) all: String,
    pub(super) index: Option<usize>,
}

/// Every match of the selector, descending into open shadow roots as Playwright's CSS engine does.
pub(super) const QUERY_ALL: &str = "((sel) => { const out = []; const walk = (root) => { \
    out.push(...root.querySelectorAll(sel)); \
    for (const el of root.querySelectorAll('*')) { if (el.shadowRoot) walk(el.shadowRoot); } }; \
    walk(document); return out; })";

impl Locator {
    fn target(&self) -> String {
        format!("{}[{}]", self.all, self.index.unwrap_or(0))
    }

    /// `locator.count()`.
    pub async fn count(&self) -> Result<usize, PageError> {
        self.page
            .evaluate("Locator.count", &format!("{}.length", self.all))
            .await
    }

    /// `locator.nth(index)`.
    pub fn nth(&self, index: usize) -> Locator {
        Locator {
            index: Some(index),
            ..self.clone()
        }
    }

    /// `locator.inner_text()`.
    pub async fn inner_text(&self) -> Result<String, PageError> {
        let found: Option<String> = self
            .page
            .evaluate(
                "Locator.inner_text",
                &format!(
                    "(() => {{ const e = {}; return e ? e.innerText : null; }})()",
                    self.target()
                ),
            )
            .await?;
        found.ok_or_else(|| {
            PageError::other(
                "Locator.inner_text",
                format!("Timeout {DEFAULT_TIMEOUT_MS}ms exceeded."),
            )
        })
    }

    /// `locator.get_attribute(name)`.
    pub async fn get_attribute(&self, name: &str) -> Result<Option<String>, PageError> {
        let name = serde_json::to_string(name).unwrap_or_default();
        self.page
            .evaluate(
                "Locator.get_attribute",
                &format!(
                    "(() => {{ const e = {}; return e ? e.getAttribute({name}) : null; }})()",
                    self.target()
                ),
            )
            .await
    }

    /// `locator.click(timeout=timeout_ms)`.
    pub async fn click(&self, timeout_ms: u64) -> Result<(), PageError> {
        const API: &str = "Locator.click";
        let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
        let centre = format!(
            "(() => {{ const e = {}; if (!e) return null; \
             if (e.scrollIntoViewIfNeeded) e.scrollIntoViewIfNeeded(true); \
             const r = e.getBoundingClientRect(); \
             if (r.width <= 0 || r.height <= 0) return null; \
             return [r.left + r.width / 2, r.top + r.height / 2]; }})()",
            self.target()
        );
        let (x, y) = loop {
            let found: Option<(f64, f64)> = self.page.evaluate(API, &centre).await?;
            if let Some(point) = found {
                break point;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(PageError::other(
                    API,
                    format!("Timeout {timeout_ms}ms exceeded."),
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let cdp = |e: CdpError| PageError::from_cdp(API, &e);
        let mouse =
            |kind: DispatchMouseEventType, button: MouseButton, buttons: i64, clicks: i64| {
                DispatchMouseEventParams::builder()
                    .r#type(kind)
                    .x(x)
                    .y(y)
                    .button(button)
                    .buttons(buttons)
                    .click_count(clicks)
                    .build()
            };
        for params in [
            mouse(DispatchMouseEventType::MouseMoved, MouseButton::None, 0, 0),
            mouse(
                DispatchMouseEventType::MousePressed,
                MouseButton::Left,
                1,
                1,
            ),
            mouse(
                DispatchMouseEventType::MouseReleased,
                MouseButton::Left,
                0,
                1,
            ),
        ] {
            let params = params.map_err(|e| PageError::other(API, e))?;
            self.page.page.execute(params).await.map_err(cdp)?;
        }
        Ok(())
    }
}
