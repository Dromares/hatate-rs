//! The network layer (DAN-1980, Phase 1.c). Behaviour is specified in `docs/net.md`.
//!
//! - [`NetClient`] is the one constructor binaries use: reqwest on rustls, the
//!   `CryptoProvider` installed explicitly, a tower stack for per-host rate
//!   limiting, retries and timeouts.
//! - [`site`] picks the headers and cookies a site needs (`core/site_access.py`).
//! - [`pacing`] is the between-search delay arithmetic (`core/rate_limit.py`).
//! - [`Secret`] and [`Cookies`] keep credentials out of `Debug`, logs and errors.
//!
//! No engine or site parser is here; that is Phase 2.

mod client;
mod config;
mod error;
mod gate;
pub mod pacing;
mod provider;
mod request;
mod retry;
mod secret;
pub mod site;
mod transport;

pub use client::NetClient;
pub use config::{DEFAULT_MAX_BODY_BYTES, MAX_REDIRECTS, NetConfig, USER_AGENT};
pub use error::{ErrorKind, NetError};
pub use gate::{HostGate, MAX_HOST_WAIT};
pub use provider::install_crypto_provider;
pub use request::{
    Body, BodyProbe, HYDRUS_KEY_HEADER, Method, NetRequest, NetResponse, Part, Redirects, RetryMode,
};
pub use retry::{RETRY_AFTER_DEFAULT, RETRY_AFTER_MAX, polite_wait, retry_after};
pub use secret::{Cookies, Secret, redact_url};

#[cfg(test)]
mod stack_tests;
