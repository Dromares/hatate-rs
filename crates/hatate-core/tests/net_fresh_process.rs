//! DAN-1909: a client built in a process where nothing installed a rustls
//! `CryptoProvider`. This file holds exactly one test, so cargo runs it in its
//! own process and no earlier test can have installed one.
//!
//! With the install removed from `NetClient::new`, this test panics with
//! "Could not automatically determine the process-level CryptoProvider".

mod common;

use std::time::Duration;

use common::{Reply, TestPki, TestServer};
use hatate_core::net::{NetClient, NetConfig, NetRequest};
use rustls::crypto::CryptoProvider;

#[tokio::test]
async fn client_builds_and_connects_in_a_fresh_process_with_no_provider_installed() {
    assert!(
        CryptoProvider::get_default().is_none(),
        "precondition: this process has no default provider yet"
    );
    // The test server is built with an explicit provider, so it installs nothing.
    let pki = TestPki::generate();
    let server = TestServer::https(&pki, |_| Reply::ok("fresh")).await;
    assert!(
        CryptoProvider::get_default().is_none(),
        "test setup must not install a provider"
    );

    let client = NetClient::new(NetConfig::default().with_root_certificate_der(pki.ca_der.clone()))
        .expect("the constructor installs the provider and builds the client");
    assert!(
        CryptoProvider::get_default().is_some(),
        "the constructor installed it"
    );

    let resp = client
        .send(NetRequest::get(&server.url("/"), Duration::from_secs(10)).unwrap())
        .await
        .expect("a real TLS request completes");
    assert_eq!(resp.text(), "fresh");

    // A second client in the same process is fine too: the install is idempotent.
    NetClient::new(NetConfig::default()).unwrap();
    hatate_core::net::install_crypto_provider();
}
