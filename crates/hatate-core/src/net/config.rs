//! Client-wide settings. Per-request values (timeout, deadline) live on the request.

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
}

impl Default for NetConfig {
    fn default() -> Self {
        NetConfig {
            user_agent: USER_AGENT.to_owned(),
            verify_tls: true,
            extra_root_certs_der: Vec::new(),
            max_redirects: MAX_REDIRECTS,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }
}

impl NetConfig {
    /// Trusts one more certificate authority (DER).
    pub fn with_root_certificate_der(mut self, der: Vec<u8>) -> Self {
        self.extra_root_certs_der.push(der);
        self
    }

    /// Turns certificate verification off. For a user's own Hydrus with a
    /// self-signed certificate; never the default.
    pub fn without_tls_verification(mut self) -> Self {
        self.verify_tls = false;
        self
    }
}
