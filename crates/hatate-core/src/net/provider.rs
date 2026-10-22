//! The rustls `CryptoProvider`, installed explicitly (DAN-1909).
//!
//! rv-rs v0.1.0 and v0.2.0 shipped with every network call panicking because no
//! process-wide provider had been installed. reqwest is built here with
//! `rustls-no-provider`, so it reads whatever default provider the process has
//! and panics when there is none. [`install_crypto_provider`] puts the `ring`
//! provider there, and [`crate::net::NetClient::new`] calls it, so a binary that
//! builds its client through the constructor cannot forget it.

use std::sync::Once;

static INSTALL: Once = Once::new();

/// Installs the `ring` provider as the process-wide default, once.
///
/// Calling it again is harmless, and so is calling it when something else
/// installed a provider first: the existing one stays and nothing panics.
/// Returns whether the process has a default provider afterwards.
pub fn install_crypto_provider() -> bool {
    INSTALL.call_once(|| {
        // `install_default` returns Err when a provider is already installed.
        // That is the harmless case, so the result is dropped on purpose.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    rustls::crypto::CryptoProvider::get_default().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_install_is_harmless() {
        assert!(install_crypto_provider());
        assert!(install_crypto_provider());
        // A direct second install by someone else reports Err, never panics.
        assert!(
            rustls::crypto::ring::default_provider()
                .install_default()
                .is_err()
        );
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
