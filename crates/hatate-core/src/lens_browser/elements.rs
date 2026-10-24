//! The Playwright calls `_one` and its helpers make that the lifecycle half did not need
//! (`query_selector_all`, `ElementHandle.bounding_box/get_attribute`, `page.mouse`,
//! `get_by_text/get_by_role`, `page.on("response")`, `wait_for_url`, `page.evaluate`), over the CDP
//! client. 5.c.
//!
//! **Cut.** `get_by_role("link", name=...)` is Playwright's role query for the one role and one
//! name form `_one` uses: an explicit `role="link"` or an `a[href]` / `area[href]`, not hidden for
//! ARIA, whose accessible name (`aria-labelledby`, then `aria-label`, then its content, then
//! `title`) contains the name case-insensitively. The rest of the accessible-name algorithm
//! (`aria-describedby`, `<input>` labels, `<svg><title>`, CSS `::before` content) is not ported.
//! `get_by_text(label, exact=True)` is Playwright's strict text query: the elements whose
//! whitespace-normalised text is the label and none of whose children's is, outside `<script>`,
//! `<style>`, `<noscript>` and `<head>`, descending into open shadow roots.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
};
use chromiumoxide::cdp::browser_protocol::network::{
    EventLoadingFailed, EventLoadingFinished, EventResponseReceived, GetResponseBodyParams,
    ResourceType,
};
use chromiumoxide::cdp::js_protocol::runtime::{
    CallArgument, CallFunctionOnParams, EvaluateParams, ReleaseObjectGroupParams, RemoteObjectId,
};
use chromiumoxide::error::CdpError;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::task::JoinHandle;

use super::page::{LensPage, Locator, PageError, QUERY_ALL};

/// The object group every handle lives in, so they can be let go together.
const HANDLE_GROUP: &str = "hatate-lens-handles";

/// `page.mouse`: where the pointer is, and whether the left button is down.
#[derive(Debug, Default, Clone, Copy)]
pub struct MouseState {
    x: f64,
    y: f64,
    left_down: bool,
}

/// A box in CSS pixels (`ElementHandle.bounding_box()`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct BoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A Playwright `ElementHandle`: one element out of the array a query returned.
#[derive(Clone)]
pub struct ElementHandle {
    page: LensPage,
    list: RemoteObjectId,
    index: usize,
}

impl ElementHandle {
    async fn call<T: DeserializeOwned>(
        &self,
        api: &str,
        body: &str,
        argument: Option<Value>,
    ) -> Result<T, PageError> {
        self.page.check(api)?;
        let mut arguments = vec![CallArgument::builder().value(self.index).build()];
        if let Some(value) = argument {
            arguments.push(CallArgument::builder().value(value).build());
        }
        let mut params = CallFunctionOnParams::new(format!("function(i, arg) {{ {body} }}"));
        params.object_id = Some(self.list.clone());
        params.arguments = Some(arguments);
        params.return_by_value = Some(true);
        let reply = self
            .page
            .page
            .execute(params)
            .await
            .map_err(|e| PageError::from_cdp(api, &e))?;
        if let Some(details) = &reply.result.exception_details {
            return Err(PageError::other(api, details.text.clone()));
        }
        let value = reply.result.result.value.clone().unwrap_or(Value::Null);
        serde_json::from_value(value).map_err(|e| {
            PageError::other(
                api,
                format!("the page returned a value that cannot be read: {e}"),
            )
        })
    }

    /// `element.bounding_box()`: `None` when the element has no box (not displayed, or gone).
    pub async fn bounding_box(&self) -> Result<Option<BoundingBox>, PageError> {
        self.call(
            "ElementHandle.bounding_box",
            "const e = this[i]; if (!e || !e.isConnected) return null; \
             if (!e.getClientRects().length) return null; \
             const r = e.getBoundingClientRect(); \
             return {x: r.x, y: r.y, width: r.width, height: r.height};",
            None,
        )
        .await
    }

    /// `element.get_attribute(name)`.
    pub async fn get_attribute(&self, name: &str) -> Result<Option<String>, PageError> {
        self.call(
            "ElementHandle.get_attribute",
            "const e = this[i]; return e ? e.getAttribute(arg) : null;",
            Some(Value::String(name.to_owned())),
        )
        .await
    }
}

/// `Page.on("response")`: the bodies of the responses `want` picked, in the order they finished.
///
/// Playwright runs the handler for one response at a time and blocks in it on `response.text()`
/// until that response has finished loading, so the next response waits behind it. The task here
/// does the same. A response whose body cannot be read (a redirect, a failed load, bytes that are
/// not UTF-8, which `response.text()` refuses) is skipped, as the handler's `except` skips it.
pub struct ResponseCapture {
    bodies: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl ResponseCapture {
    /// `len(bodies)`.
    pub fn len(&self) -> usize {
        self.bodies.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `bodies[from:]`.
    pub fn since(&self, from: usize) -> Vec<String> {
        let bodies = self.bodies.lock().unwrap_or_else(|p| p.into_inner());
        bodies.get(from..).unwrap_or_default().to_vec()
    }

    /// `page.remove_listener("response", on_response)`.
    pub fn stop(self) {
        self.task.abort();
    }
}

impl Drop for ResponseCapture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Playwright's `Mouse` button mask for the left button.
const LEFT_MASK: i64 = 1;

impl LensPage {
    /// Starts listening for responses. `want(resource_type_is_document_xhr_or_fetch_url, body)`:
    /// the URL test and the body test are the caller's (`"/search" in response.url` and
    /// `SOURCE_HINT_RE.search(body)`).
    pub async fn capture_responses(
        &self,
        url_wanted: impl Fn(&str) -> bool + Send + 'static,
        body_wanted: impl Fn(&str) -> bool + Send + 'static,
    ) -> Result<ResponseCapture, PageError> {
        const API: &str = "Page.on";
        self.check(API)?;
        let cdp = |e: CdpError| PageError::from_cdp(API, &e);
        let mut received = self
            .page
            .event_listener::<EventResponseReceived>()
            .await
            .map_err(cdp)?;
        let mut finished = self
            .page
            .event_listener::<EventLoadingFinished>()
            .await
            .map_err(cdp)?;
        let mut failed = self
            .page
            .event_listener::<EventLoadingFailed>()
            .await
            .map_err(cdp)?;
        let bodies: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = Arc::clone(&bodies);
        let page = self.page.clone();
        let task = tokio::spawn(async move {
            // Responses waiting for their body, oldest first; the ids that finished or failed.
            let mut pending: VecDeque<chromiumoxide::cdp::browser_protocol::network::RequestId> =
                VecDeque::new();
            let mut done: std::collections::HashMap<String, bool> = Default::default();
            loop {
                // Read the oldest response whose outcome is known, before listening again.
                while let Some(id) = pending.front().cloned() {
                    let Some(&loaded) = done.get(id.as_ref()) else {
                        break;
                    };
                    pending.pop_front();
                    done.remove(id.as_ref());
                    if !loaded {
                        continue;
                    }
                    let Ok(reply) = page.execute(GetResponseBodyParams::new(id)).await else {
                        continue;
                    };
                    let bytes = if reply.result.base64_encoded {
                        match base64::engine::general_purpose::STANDARD.decode(&reply.result.body) {
                            Ok(bytes) => bytes,
                            Err(_) => continue,
                        }
                    } else {
                        reply.result.body.clone().into_bytes()
                    };
                    let Ok(text) = String::from_utf8(bytes) else {
                        continue;
                    };
                    if body_wanted(&text) {
                        sink.lock().unwrap_or_else(|p| p.into_inner()).push(text);
                    }
                }
                tokio::select! {
                    Some(event) = received.next() => {
                        if matches!(event.r#type, ResourceType::Document | ResourceType::Xhr | ResourceType::Fetch)
                            && url_wanted(&event.response.url)
                        {
                            pending.push_back(event.request_id.clone());
                        }
                    }
                    Some(event) = finished.next() => {
                        done.insert(event.request_id.as_ref().to_owned(), true);
                    }
                    Some(event) = failed.next() => {
                        done.insert(event.request_id.as_ref().to_owned(), false);
                    }
                    else => return,
                }
            }
        });
        Ok(ResponseCapture { bodies, task })
    }

    /// `page.evaluate(expression)` for an expression that yields JSON.
    pub async fn evaluate_value(&self, expression: &str) -> Result<Value, PageError> {
        let api = "Page.evaluate";
        self.check(api)?;
        let reply = self
            .page
            .execute(
                EvaluateParams::builder()
                    .expression(expression)
                    .return_by_value(true)
                    .await_promise(true)
                    .build()
                    .map_err(|e| PageError::other(api, e))?,
            )
            .await
            .map_err(|e| PageError::from_cdp(api, &e))?;
        if let Some(details) = &reply.result.exception_details {
            let text = details
                .exception
                .as_ref()
                .and_then(|e| e.description.clone())
                .unwrap_or_else(|| details.text.clone());
            return Err(PageError::other(api, text));
        }
        Ok(reply.result.result.value.clone().unwrap_or(Value::Null))
    }

    /// `page.wait_for_url(pattern, timeout=timeout_ms)`: waits until the page's URL matches and the
    /// navigation that brought it there has loaded.
    pub async fn wait_for_url(
        &self,
        matches: impl Fn(&str) -> bool,
        timeout_ms: u64,
    ) -> Result<(), PageError> {
        const API: &str = "Page.wait_for_url";
        self.check(API)?;
        let work = async {
            loop {
                self.check(API)?;
                if matches(&self.url().await) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            // The `load` event of the page that matched.
            loop {
                self.check(API)?;
                if let Ok(Value::String(state)) = self.evaluate_value("document.readyState").await
                    && state == "complete"
                {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        };
        match tokio::time::timeout(Duration::from_millis(timeout_ms), work).await {
            Ok(result) => result,
            // Python's text for this one has no `Page.wait_for_url:` in front of it.
            Err(_) => Err(PageError::bare(format!("Timeout {timeout_ms}ms exceeded."))),
        }
    }

    /// `page.query_selector_all(selector)`: every match, open shadow roots included.
    pub async fn query_selector_all(
        &self,
        selector: &str,
    ) -> Result<Vec<ElementHandle>, PageError> {
        const API: &str = "Page.query_selector_all";
        self.check(API)?;
        let quoted = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_owned());
        let reply = self
            .page
            .execute(
                EvaluateParams::builder()
                    .expression(format!("{QUERY_ALL}({quoted})"))
                    .return_by_value(false)
                    .object_group(HANDLE_GROUP)
                    .build()
                    .map_err(|e| PageError::other(API, e))?,
            )
            .await
            .map_err(|e| PageError::from_cdp(API, &e))?;
        if let Some(details) = &reply.result.exception_details {
            return Err(PageError::other(API, details.text.clone()));
        }
        let Some(list) = reply.result.result.object_id.clone() else {
            return Ok(Vec::new());
        };
        let mut length = CallFunctionOnParams::new("function() { return this.length; }");
        length.object_id = Some(list.clone());
        length.return_by_value = Some(true);
        let count = self
            .page
            .execute(length)
            .await
            .map_err(|e| PageError::from_cdp(API, &e))?
            .result
            .result
            .value
            .as_ref()
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        Ok((0..count)
            .map(|index| ElementHandle {
                page: self.clone(),
                list: list.clone(),
                index,
            })
            .collect())
    }

    /// Lets go of every handle `query_selector_all` made.
    pub async fn release_handles(&self) {
        let _ = self
            .page
            .execute(ReleaseObjectGroupParams::new(HANDLE_GROUP))
            .await;
    }

    async fn dispatch_mouse(
        &self,
        api: &str,
        kind: DispatchMouseEventType,
        (x, y): (f64, f64),
        button: MouseButton,
        down: bool,
        click_count: Option<i64>,
    ) -> Result<(), PageError> {
        let mut builder = DispatchMouseEventParams::builder()
            .r#type(kind)
            .x(x)
            .y(y)
            .button(button)
            .buttons(if down { LEFT_MASK } else { 0 })
            .modifiers(0)
            .force(if down { 0.5 } else { 0.0 });
        if let Some(count) = click_count {
            builder = builder.click_count(count);
        }
        let params = builder.build().map_err(|e| PageError::other(api, e))?;
        self.page
            .execute(params)
            .await
            .map(|_| ())
            .map_err(|e| PageError::from_cdp(api, &e))
    }

    /// `page.mouse.move(x, y, steps=steps)`.
    pub async fn mouse_move(&self, x: f64, y: f64, steps: u32) -> Result<(), PageError> {
        const API: &str = "Mouse.move";
        self.check(API)?;
        let (from_x, from_y, down) = {
            let mut state = self.mouse.lock().unwrap_or_else(|p| p.into_inner());
            let from = (state.x, state.y, state.left_down);
            state.x = x;
            state.y = y;
            from
        };
        for i in 1..=steps {
            let middle_x = from_x + (x - from_x) * (f64::from(i) / f64::from(steps));
            let middle_y = from_y + (y - from_y) * (f64::from(i) / f64::from(steps));
            let button = if down {
                MouseButton::Left
            } else {
                MouseButton::None
            };
            self.dispatch_mouse(
                API,
                DispatchMouseEventType::MouseMoved,
                (middle_x, middle_y),
                button,
                down,
                None,
            )
            .await?;
        }
        Ok(())
    }

    /// `page.mouse.down()`.
    pub async fn mouse_down(&self) -> Result<(), PageError> {
        const API: &str = "Mouse.down";
        self.check(API)?;
        let (x, y) = {
            let mut state = self.mouse.lock().unwrap_or_else(|p| p.into_inner());
            state.left_down = true;
            (state.x, state.y)
        };
        self.dispatch_mouse(
            API,
            DispatchMouseEventType::MousePressed,
            (x, y),
            MouseButton::Left,
            true,
            Some(1),
        )
        .await
    }

    /// `page.mouse.up()`.
    pub async fn mouse_up(&self) -> Result<(), PageError> {
        const API: &str = "Mouse.up";
        self.check(API)?;
        let (x, y) = {
            let mut state = self.mouse.lock().unwrap_or_else(|p| p.into_inner());
            state.left_down = false;
            (state.x, state.y)
        };
        self.dispatch_mouse(
            API,
            DispatchMouseEventType::MouseReleased,
            (x, y),
            MouseButton::Left,
            false,
            Some(1),
        )
        .await
    }

    /// `page.get_by_text(text, exact=True)`.
    pub fn get_by_text_exact(&self, text: &str) -> Locator {
        let quoted = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned());
        Locator {
            page: self.clone(),
            all: format!("({TEXT_QUERY})({quoted})"),
            index: None,
        }
    }

    /// `page.get_by_role("link", name=name)`.
    pub fn get_by_link(&self, name: &str) -> Locator {
        let quoted = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".to_owned());
        Locator {
            page: self.clone(),
            all: format!("({LINK_QUERY})({quoted})"),
            index: None,
        }
    }
}

/// The strict text query (`internal:text="..."s`): see the module notes.
const TEXT_QUERY: &str = concat!(
    "(label) => { ",
    "const norm = (s) => s.replace(/[\\u200b\\u00ad]/g, '').trim().replace(/\\s+/g, ' '); ",
    "const want = norm(label); const cache = new Map(); ",
    "const skip = (e) => e.nodeName === 'SCRIPT' || e.nodeName === 'NOSCRIPT' || \
     e.nodeName === 'STYLE' || (e.ownerDocument.head && e.ownerDocument.head.contains(e)); ",
    "const text = (root) => { let v = cache.get(root); if (v !== undefined) return v; v = ''; ",
    "  if (!skip(root)) { ",
    "    if (root instanceof HTMLInputElement && \
     (root.type === 'submit' || root.type === 'button' || root.type === 'reset')) { v = root.value; } ",
    "    else { for (let c = root.firstChild; c; c = c.nextSibling) { ",
    "      if (c.nodeType === 3) v += c.nodeValue || ''; ",
    "      else if (c.nodeType === 8) continue; ",
    "      else if (c.nodeType === 1) v += text(c); } ",
    "      if (root.shadowRoot) v += text(root.shadowRoot); } } ",
    "  cache.set(root, v); return v; }; ",
    "const matches = (e) => { if (skip(e)) return false; if (norm(text(e)) !== want) return false; ",
    "  for (let c = e.firstChild; c; c = c.nextSibling) { \
     if (c.nodeType === 1 && norm(text(c)) === want) return false; } ",
    "  if (e.shadowRoot && norm(text(e.shadowRoot)) === want) return false; return true; }; ",
    "const out = []; const walk = (root) => { ",
    "  for (const e of root.querySelectorAll('*')) { if (matches(e)) out.push(e); ",
    "    if (e.shadowRoot) walk(e.shadowRoot); } }; ",
    "walk(document); return out; }"
);

/// The link role query with a name (`internal:role=link[name="..."i]`): see the module notes.
const LINK_QUERY: &str = concat!(
    "(name) => { ",
    "const norm = (s) => s.replace(/[\\u200b\\u00ad]/g, '').trim().replace(/\\s+/g, ' '); ",
    "const want = norm(name).toLowerCase(); ",
    "const hidden = (e) => { for (let n = e; n; n = n.parentElement) { \
     const s = getComputedStyle(n); \
     if (!s || s.display === 'none' || n.getAttribute('aria-hidden') === 'true') return true; \
     if (n === e && s.visibility !== 'visible') return true; } return false; }; ",
    "const role = (e) => { const r = (e.getAttribute('role') || '').trim().split(/\\s+/)[0]; \
     if (r) return r.toLowerCase(); \
     return (e.nodeName === 'A' || e.nodeName === 'AREA') && e.hasAttribute('href') ? 'link' : ''; }; ",
    "const content = (n) => { let v = ''; for (let c = n.firstChild; c; c = c.nextSibling) { \
     if (c.nodeType === 3) v += c.nodeValue || ''; \
     else if (c.nodeType === 1) { if (hidden(c)) continue; \
       const d = getComputedStyle(c).display; const inline = d.startsWith('inline'); \
       v += (inline ? '' : ' ') + (c.nodeName === 'IMG' ? (c.getAttribute('alt') || '') : content(c)) \
         + (inline ? '' : ' '); } } return v; }; ",
    "const named = (e) => { const by = e.getAttribute('aria-labelledby'); \
     if (by) { const parts = by.split(' ').filter(Boolean).map(id => document.getElementById(id)) \
       .filter(Boolean).map(x => content(x) || x.textContent || ''); if (parts.length) return parts.join(' '); } ",
    "  const label = e.getAttribute('aria-label'); if (label !== null && label.trim()) return label; ",
    "  const body = content(e); if (body.trim()) return body; ",
    "  return e.getAttribute('title') || ''; }; ",
    "const out = []; const walk = (root) => { for (const e of root.querySelectorAll('*')) { ",
    "  if (role(e) === 'link' && !hidden(e) && norm(named(e)).toLowerCase().includes(want)) out.push(e); ",
    "  if (e.shadowRoot) walk(e.shadowRoot); } }; ",
    "walk(document); return out; }"
);
