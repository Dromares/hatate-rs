//! Client-wide settings. Per-request values (timeout, deadline) live on the request.

use std::net::SocketAddr;

/// The app's own identity (`core/net.py:50`). Sites that need a browser keep
/// their own strings (`net::site`).
pub const USER_AGENT: &str = "hatate-linux/1.0 (+https://github.com/nostrenz/hatate-iqdb-tagger)";

/// How many redirects a request follows before giving up (the `requests` default).
pub const MAX_REDIRECTS: usize = 30;

/// Default cap on one response body. The Python app has none; this only stops a
/// server that never ends from filling memory before the deadline fires.
pub const DEFAULT_MAX_BODY_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct NetConfig {
    /// `User-Agent` sent when a request sets none.
    pub user_agent: String,
    /// Verify server certificates. Only Hydrus has a switch for this
    /// (`HydrusSettings.verify_ssl`, `core/config.py:155`); everything else keeps `true`.
    pub verify_tls: bool,
    /// Extra trust anchors, DER-encoded, added to the system trust store.
    pub extra_root_certs_der: Vec<Vec<u8>>,
    pub max_redirects: usize,
    pub max_body_bytes: usize,
    /// Where a host name connects to instead of its DNS answer (host lower-cased). The URL, the
    /// `Host` header and the TLS server name stay the real ones; only the socket goes elsewhere.
    /// Set only by a caller constructing the client in code (tests); the binaries have no
    /// setting, flag or environment variable that fills it.
    pub host_overrides: Vec<(String, SocketAddr)>,
}

impl Default for NetConfig {
    fn default() -> Self {
        NetConfig {
            user_agent: USER_AGENT.to_owned(),
            verify_tls: true,
            extra_root_certs_der: Vec::new(),
            max_redirects: MAX_REDIRECTS,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            host_overrides: Vec::new(),
        }
    }
}

impl NetConfig {
    /// Trusts one more certificate authority (DER).
    pub fn with_root_certificate_der(mut self, der: Vec<u8>) -> Self {
        self.extra_root_certs_der.push(der);
        self
    }

    /// Connects `host` to `addr` instead of resolving it (DAN-2058). A URL with no port of its own
    /// uses `addr`'s port; a URL that names one keeps it. Requests still carry the real host in
    /// the URL, the `Host` header and the TLS handshake, so what a server records is what the
    /// site would have seen.
    pub fn with_host_override(mut self, host: &str, addr: SocketAddr) -> Self {
        self.host_overrides.push((host.to_ascii_lowercase(), addr));
        self
    }

    /// Turns certificate verification off. For a user's own Hydrus with a
    /// self-signed certificate; never the default.
    pub fn without_tls_verification(mut self) -> Self {
        self.verify_tls = false;
        self
    }
}
