//! The server's lifecycle (`McpServerController`) and the hyper layer under it.
//!
//! `start` binds before it returns, so a busy port is reported through `last_error` at once
//! (Python waits up to `BIND_TIMEOUT_SECONDS` for uvicorn to flip `started`, and gives up early
//! when its thread dies; a bind that fails is the same message either way).

use std::convert::Infallible;
use std::future::Future;
use std::net::{SocketAddr, TcpListener as StdListener};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::thread::JoinHandle;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Body, Frame, Incoming};
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::watch;

use super::protocol::{Core, MAX_BODY_BYTES, Reply, ReplyBody, Request};
use super::tools::{TOOLS, gate};
use super::{HOST, McpError, handlers::McpHandlers};
use crate::config::McpSettings;
use crate::error::ErrorKind;

/// How long `start` waits for the socket to be listening (`BIND_TIMEOUT_SECONDS`).
pub const BIND_TIMEOUT_SECONDS: f64 = 5.0;
/// How long `stop` waits for the server thread (`join(timeout=5.0)`).
const STOP_JOIN_SECONDS: f64 = 5.0;

/// `_mcp_top_level_import_succeeds()`: whether the server is available. Python asks whether the
/// optional `mcp` package imports; the Rust server is part of this build, so it always is.
pub fn mcp_top_level_import_succeeds() -> bool {
    true
}

/// Owns the running server. Every failure (a blank token, the port already in use) is logged and
/// recorded on `last_error` rather than returned as an error: `start` is called from the GUI's
/// event loop, and a failure there must not take the app down with it.
pub struct McpServerController {
    handlers: Arc<dyn McpHandlers>,
    thread: Option<JoinHandle<()>>,
    shutdown: Option<watch::Sender<bool>>,
    last_error: Option<String>,
    last_failure: Option<McpError>,
}

impl McpServerController {
    pub fn new(handlers: Arc<dyn McpHandlers>) -> Self {
        Self {
            handlers,
            thread: None,
            shutdown: None,
            last_error: None,
            last_failure: None,
        }
    }

    /// `last_error`: the text the Settings page shows.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// The kind behind `last_error`, for a caller that decides what to show or retry.
    pub fn last_error_kind(&self) -> Option<ErrorKind> {
        self.last_failure.as_ref().map(McpError::kind)
    }

    /// What the Settings page should show for the last failure: what failed and what to do. No
    /// token character, path or OS text (DAN-2116). `last_error` keeps Python's text for parity;
    /// that text can carry a token character, so a UI shows this one.
    pub fn last_error_user_message(&self) -> Option<String> {
        self.last_failure.as_ref().map(McpError::user_message)
    }

    /// The raw text for a log: the same string as `last_error`.
    pub fn last_error_detail(&self) -> Option<String> {
        self.last_failure.as_ref().map(McpError::detail)
    }

    /// `running`.
    pub fn running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    fn fail(&mut self, kind: ErrorKind, user_message: &str, message: String) -> bool {
        tracing::error!("{message}");
        self.last_failure = Some(McpError::start_failure(kind, user_message, message.clone()));
        self.last_error = Some(message);
        false
    }

    /// Starts the server if enabled and not already running. Returns whether it is running once
    /// this call returns.
    pub fn start(&mut self, settings: &McpSettings) -> bool {
        if self.running() {
            return true;
        }
        if !settings.enabled {
            return false;
        }
        let token = settings.token.expose().to_owned();
        if token.is_empty() {
            return self.fail(
                ErrorKind::McpServerStart,
                "The MCP server needs a bearer token. Set one in Settings \u{2192} MCP.",
                "Blank bearer token: refusing to start the MCP server".to_owned(),
            );
        }
        if let Some((index, c)) = format!("Bearer {token}")
            .chars()
            .enumerate()
            .find(|(_, c)| u32::from(*c) > 0xff)
        {
            let shown = if u32::from(c) > 0xffff {
                format!("\\U{:08x}", u32::from(c))
            } else {
                format!("\\u{:04x}", u32::from(c))
            };
            return self.fail(
                ErrorKind::McpServerStart,
                "The MCP bearer token has a character that cannot be sent in an HTTP header. Use a token of plain letters and digits in Settings \u{2192} MCP.",
                format!(
                    "Could not start the MCP server: 'latin-1' codec can't encode character '{shown}' in position {index}: ordinal not in range(256)"
                ),
            );
        }
        let port = settings.port;
        let Ok(port16) = u16::try_from(port) else {
            return self.fail(
                ErrorKind::McpServerStart,
                "The MCP port is not valid. Pick a port between 1 and 65535 in Settings \u{2192} MCP.",
                "Could not start the MCP server: bind(): port must be 0-65535.".to_owned(),
            );
        };
        let listener = match StdListener::bind((HOST, port16))
            .and_then(|l| l.set_nonblocking(true).map(|()| l))
        {
            Ok(l) => l,
            Err(error) => {
                tracing::debug!("MCP bind failed: {error}");
                return self.fail(
                    ErrorKind::McpPortBusy,
                    "The MCP port is already in use. Pick another port in Settings \u{2192} MCP.",
                    format!(
                        "MCP server did not start listening on port {port} within {BIND_TIMEOUT_SECONDS:.0}s (port already in use?)"
                    ),
                );
            }
        };
        let core = Arc::new(Core::new(Arc::clone(&self.handlers), &token));
        let (tx, rx) = watch::channel(false);
        let spawned = std::thread::Builder::new()
            .name("mcp-server".into())
            .spawn(move || serve_thread(listener, core, rx));
        let thread = match spawned {
            Ok(t) => t,
            Err(error) => {
                tracing::debug!("MCP thread failed: {error}");
                return self.fail(
                    ErrorKind::McpServerStart,
                    "The MCP server could not start. Try again, or restart Hatate.",
                    "Could not start the MCP server: could not start a thread".to_owned(),
                );
            }
        };
        self.thread = Some(thread);
        self.shutdown = Some(tx);
        self.last_error = None;
        self.last_failure = None;
        let mut enabled: Vec<&str> = TOOLS
            .iter()
            .filter(|t| gate(t.name, settings).is_ok_and(|g| g.is_none()))
            .map(|t| t.name)
            .collect();
        enabled.sort_unstable();
        tracing::info!(
            "MCP server listening on {HOST}:{port} (loopback only, enabled tools: {})",
            enabled.join(", ")
        );
        true
    }

    /// Stops the server if running. Safe to call unconditionally, including when it was never
    /// started (from a window's close). `shutdown_invoker` is true for that close; `restart`
    /// passes false, because the invoker lives as long as the window and `restart` on a
    /// controller that was never started must not kill it (DAN-912).
    pub fn stop(&mut self, shutdown_invoker: bool) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(true);
        }
        if let Some(thread) = self.thread.take() {
            let deadline = std::time::Instant::now() + Duration::from_secs_f64(STOP_JOIN_SECONDS);
            while !thread.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
        if shutdown_invoker {
            self.handlers.shutdown_invoker();
        }
    }

    /// Applies a settings change (port, token, enabled) that `start` alone cannot pick up on a
    /// running server. Gating and `dry_run` are read live on every call and need no restart.
    pub fn restart(&mut self, settings: &McpSettings) -> bool {
        self.stop(false);
        self.start(settings)
    }
}

impl Drop for McpServerController {
    fn drop(&mut self) {
        self.stop(false);
    }
}

fn serve_thread(listener: StdListener, core: Arc<Core>, shutdown: watch::Receiver<bool>) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(error) => {
            tracing::error!("MCP server runtime: {error}");
            return;
        }
    };
    runtime.block_on(accept_loop(listener, core, shutdown));
    runtime.shutdown_timeout(Duration::from_secs(1));
}

async fn accept_loop(listener: StdListener, core: Arc<Core>, mut shutdown: watch::Receiver<bool>) {
    let Ok(listener) = TcpListener::from_std(listener) else {
        return;
    };
    loop {
        tokio::select! {
            _ = shutdown.changed() => return,
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else { continue };
                let core = Arc::clone(&core);
                let shutdown = shutdown.clone();
                tokio::spawn(connection(stream, peer, core, shutdown));
            }
        }
    }
}

async fn connection(
    stream: tokio::net::TcpStream,
    _peer: SocketAddr,
    core: Arc<Core>,
    shutdown: watch::Receiver<bool>,
) {
    let io = TokioIo::new(stream);
    let held = shutdown.clone();
    let service = service_fn(move |req: hyper::Request<Incoming>| {
        let core = Arc::clone(&core);
        let held = held.clone();
        async move { Ok::<_, Infallible>(respond(core, req, held).await) }
    });
    let conn = hyper::server::conn::http1::Builder::new()
        .keep_alive(true)
        .serve_connection(io, service);
    let mut shutdown = shutdown;
    tokio::select! {
        _ = conn => {}
        _ = shutdown.changed() => {}
    }
}

type ReplyBodyType = ServerBody;

async fn respond(
    core: Arc<Core>,
    req: hyper::Request<Incoming>,
    shutdown: watch::Receiver<bool>,
) -> hyper::Response<ReplyBodyType> {
    let (parts, body) = req.into_parts();
    let headers: Vec<(String, Vec<u8>)> = parts
        .headers
        .iter()
        .map(|(n, v)| (n.as_str().to_owned(), v.as_bytes().to_vec()))
        .collect();
    let body = match Limited::new(body, MAX_BODY_BYTES).collect().await {
        Ok(collected) => collected.to_bytes().to_vec(),
        Err(_) => {
            return build(
                Reply {
                    status: 413,
                    headers: vec![("content-type".into(), "text/plain; charset=utf-8".into())],
                    body: ReplyBody::Bytes(b"Request Entity Too Large".to_vec()),
                },
                shutdown,
                Arc::clone(&core),
            );
        }
    };
    let request = Request {
        method: parts.method.as_str().to_owned(),
        path: parts.uri.path().to_owned(),
        query: parts.uri.query().map(str::to_owned),
        headers,
        body,
    };
    let reply = core.handle(request).await;
    build(reply, shutdown, Arc::clone(&core))
}

fn build(
    reply: Reply,
    shutdown: watch::Receiver<bool>,
    core: Arc<Core>,
) -> hyper::Response<ServerBody> {
    let mut builder = hyper::Response::builder().status(reply.status);
    for (name, value) in &reply.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let body = match reply.body {
        ReplyBody::Bytes(bytes) => ServerBody::Full(Full::new(Bytes::from(bytes))),
        ReplyBody::HeldOpen(session) => ServerBody::HeldOpen(
            Box::pin(wait_for_shutdown(shutdown)),
            StreamSlot { core, session },
        ),
    };
    builder
        .body(body)
        .unwrap_or_else(|_| hyper::Response::new(ServerBody::Full(Full::new(Bytes::new()))))
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    let _ = shutdown.changed().await;
}

/// A response body: a whole buffer, or an event stream that never has an event and ends when the
/// server stops.
pub enum ServerBody {
    Full(Full<Bytes>),
    HeldOpen(Pin<Box<dyn Future<Output = ()> + Send>>, StreamSlot),
}

impl Body for ServerBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        match self.get_mut() {
            ServerBody::Full(full) => Pin::new(full).poll_frame(cx),
            ServerBody::HeldOpen(wait, _) => match wait.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

/// Held by a `GET` stream's body; dropping the body (the client hung up, or the server is
/// stopping) frees the session's one stream slot.
pub struct StreamSlot {
    core: Arc<Core>,
    session: String,
}

impl Drop for StreamSlot {
    fn drop(&mut self) {
        self.core.release_get_stream(&self.session);
    }
}
