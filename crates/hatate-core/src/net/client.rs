//! `NetClient`: the one constructor the binaries use.

use std::fmt;
use std::sync::Arc;

use tower::util::BoxCloneSyncService;
use tower::{Service, ServiceBuilder, ServiceExt};

use super::config::NetConfig;
use super::error::NetError;
use super::gate::{AdmitLayer, HostGate, PaceLayer};
use super::provider::install_crypto_provider;
use super::request::{NetRequest, NetResponse};
use super::retry::RetryRules;
use super::transport::Transport;

type Stack = BoxCloneSyncService<NetRequest, NetResponse, NetError>;

/// Puts the rate limiting, retry and timeout rules around `inner`.
///
/// Outermost first: `Admit` (wait out a host's reset once, start the deadline
/// clock) -> `Retry` -> `Pace` (minimum gap and rate-limit headers, per attempt)
/// -> `inner`.
pub(crate) fn stack<S>(inner: S, gate: &HostGate) -> Stack
where
    S: Service<NetRequest, Response = NetResponse, Error = NetError>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Future: Send + 'static,
{
    BoxCloneSyncService::new(
        ServiceBuilder::new()
            .layer(AdmitLayer { gate: gate.clone() })
            .retry(RetryRules::default())
            .layer(PaceLayer { gate: gate.clone() })
            .service(inner),
    )
}

/// An HTTP client on rustls with the net-layer rules applied.
///
/// Cheap to clone; clones share the connection pool and the per-host state.
#[derive(Clone)]
pub struct NetClient {
    stack: Stack,
    gate: HostGate,
    transport: Option<Transport>,
    cfg: Arc<NetConfig>,
}

impl NetClient {
    /// Builds the client. Installs the rustls `CryptoProvider` first (once; a
    /// second install is harmless), so no caller has to remember it (DAN-1909).
    pub fn new(cfg: NetConfig) -> Result<Self, NetError> {
        install_crypto_provider();
        let cfg = Arc::new(cfg);
        let transport = Transport::new(cfg.clone())?;
        let gate = HostGate::new();
        Ok(NetClient {
            stack: stack(transport.clone(), &gate),
            gate,
            transport: Some(transport),
            cfg,
        })
    }

    /// Runs `req` through the stack and reads the whole response.
    pub async fn send(&self, req: NetRequest) -> Result<NetResponse, NetError> {
        let mut stack = self.stack.clone();
        stack.ready().await?;
        stack.call(req).await
    }

    /// Closes every pooled connection and forgets every host's rate-limit state
    /// (`core/net.py:86-93`).
    pub fn close_all(&self) -> Result<(), NetError> {
        self.gate.clear();
        match &self.transport {
            Some(t) => t.reset_pool(),
            None => Ok(()),
        }
    }

    /// The per-host state, for callers that want to ask "is this host closed?".
    pub fn gate(&self) -> &HostGate {
        &self.gate
    }

    #[cfg(test)]
    pub(crate) fn with_service<S>(inner: S, cfg: NetConfig) -> Self
    where
        S: Service<NetRequest, Response = NetResponse, Error = NetError>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Future: Send + 'static,
    {
        let gate = HostGate::new();
        NetClient {
            stack: stack(inner, &gate),
            gate,
            transport: None,
            cfg: Arc::new(cfg),
        }
    }
}

impl fmt::Debug for NetClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetClient")
            .field("cfg", &self.cfg)
            .finish_non_exhaustive()
    }
}
