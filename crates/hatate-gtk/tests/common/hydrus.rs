//! A scripted Hydrus for the window tests: the core crate's `TestServer` (plain http, or https
//! with a throw-away CA) run on its own thread and runtime, so the GTK main thread stays free to
//! pump. Every access key is a synthetic placeholder and the only address is `127.0.0.1`.

#![allow(dead_code)]

#[path = "../../../hatate-core/tests/common/net_server.rs"]
pub mod net_server;

pub use net_server::{Recorded, Reply, TestPki, TestServer};
use std::sync::{Arc, Mutex};

/// A synthetic access key: 64 zeros.
pub const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";

pub struct Hydrus {
    /// `scheme://127.0.0.1:port`
    pub url: String,
    /// The throw-away CA of an https server (DER), for a client that must trust it.
    pub ca_der: Option<Vec<u8>>,
    requests: Arc<Mutex<Vec<Recorded>>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Hydrus {
    pub fn http(handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Hydrus {
        Hydrus::start(false, &[], handler)
    }

    pub fn https(handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Hydrus {
        Hydrus::start(true, &[], handler)
    }

    /// An https server whose certificate is also valid for `names`: the site hosts a test
    /// reaches through `NetConfig::with_host_override`.
    pub fn https_for(
        names: &[&str],
        handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static,
    ) -> Hydrus {
        Hydrus::start(true, names, handler)
    }

    /// The socket the server listens on.
    pub fn addr(&self) -> std::net::SocketAddr {
        self.url
            .split("://")
            .nth(1)
            .and_then(|a| a.trim_end_matches('/').parse().ok())
            .expect("a socket address")
    }

    fn start(
        tls: bool,
        names: &[&str],
        handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static,
    ) -> Hydrus {
        let (info_tx, info_rx) = std::sync::mpsc::channel();
        let pki = tls.then(|| TestPki::generate_for(names));
        let ca_der = pki.as_ref().map(|p| p.ca_der.clone());
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("runtime");
            rt.block_on(async move {
                let server = if let Some(pki) = &pki {
                    TestServer::https(pki, handler).await
                } else {
                    TestServer::http(handler).await
                };
                let _ = info_tx.send((server.url(""), server.requests.clone()));
                let _ = stopped.await;
            });
        });
        let (url, requests) = info_rx.recv().expect("the scripted Hydrus started");
        Hydrus {
            url,
            ca_der,
            requests,
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    pub fn recorded(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }

    /// `METHOD /path` of each request, query dropped, in order.
    pub fn calls(&self) -> Vec<String> {
        self.recorded()
            .iter()
            .map(|q| format!("{} {}", q.method, q.target.split('?').next().unwrap_or("")))
            .collect()
    }
}

impl Drop for Hydrus {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn json_reply(body: serde_json::Value) -> Reply {
    Reply::ok(body.to_string()).header("Content-Type", "application/json")
}
