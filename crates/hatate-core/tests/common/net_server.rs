//! A tiny local HTTP/1.1 server for the net tests, plain or over TLS.
//!
//! The TLS certificates are generated here, per test run (a throw-away CA and a
//! leaf for 127.0.0.1 / localhost). Nothing is committed and nothing is shared.

#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair, KeyUsagePurpose, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

/// One request as the server saw it.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub target: String,
    /// Header names lower-cased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub at: Instant,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// What the server answers.
#[derive(Clone, Debug)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Pause before the response head is sent.
    pub delay: Duration,
    /// Send the body in pieces of this size with this pause between them.
    pub trickle: Option<(usize, Duration)>,
    /// Close the connection without answering (a network error).
    pub abort: bool,
    /// The text of the `Content-Length` header, when it is not the body's length (a HEAD answer
    /// that states a size, or a value no real file has).
    pub content_length: Option<String>,
    /// Close the connection once the body is written, whatever `content_length` promised (a body
    /// that breaks off mid-way).
    pub close_after: bool,
}

impl Reply {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Reply {
            status,
            headers: Vec::new(),
            body: body.into(),
            delay: Duration::ZERO,
            trickle: None,
            abort: false,
            content_length: None,
            close_after: false,
        }
    }

    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self::new(200, body)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn aborted() -> Self {
        let mut reply = Self::new(0, "");
        reply.abort = true;
        reply
    }

    /// Closes the connection after the body: with a `content_length` longer than the body, the
    /// client sees the body break off.
    pub fn closing(mut self) -> Self {
        self.close_after = true;
        self
    }

    pub fn content_length(mut self, text: &str) -> Self {
        self.content_length = Some(text.to_owned());
        self
    }

    pub fn trickled(mut self, piece: usize, every: Duration) -> Self {
        self.trickle = Some((piece, every));
        self
    }
}

pub type Handler = Arc<dyn Fn(&Recorded) -> Reply + Send + Sync>;

pub struct TestServer {
    pub addr: SocketAddr,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
    pub connections: Arc<AtomicUsize>,
    scheme: &'static str,
    task: JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestServer {
    pub async fn http(handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static) -> Self {
        Self::start(Arc::new(handler), None).await
    }

    pub async fn https(
        pki: &TestPki,
        handler: impl Fn(&Recorded) -> Reply + Send + Sync + 'static,
    ) -> Self {
        Self::start(
            Arc::new(handler),
            Some(TlsAcceptor::from(pki.server_config.clone())),
        )
        .await
    }

    async fn start(handler: Handler, tls: Option<TlsAcceptor>) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let requests: Arc<Mutex<Vec<Recorded>>> = Arc::default();
        let connections = Arc::new(AtomicUsize::new(0));
        let scheme = if tls.is_some() { "https" } else { "http" };
        let task = {
            let requests = requests.clone();
            let connections = connections.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        return;
                    };
                    connections.fetch_add(1, Ordering::SeqCst);
                    let handler = handler.clone();
                    let requests = requests.clone();
                    let tls = tls.clone();
                    tokio::spawn(async move {
                        match tls {
                            Some(acceptor) => {
                                // A failed handshake (an untrusted client) just ends the connection.
                                if let Ok(stream) = acceptor.accept(stream).await {
                                    serve(stream, handler, requests).await;
                                }
                            }
                            None => serve(stream, handler, requests).await,
                        }
                    });
                }
            })
        };
        TestServer {
            addr,
            requests,
            connections,
            scheme,
            task,
        }
    }

    /// `scheme://127.0.0.1:port/path`
    pub fn url(&self, path: &str) -> String {
        format!("{}://127.0.0.1:{}{path}", self.scheme, self.addr.port())
    }

    /// The same server under the name `localhost`: a different host to the client.
    pub fn url_localhost(&self, path: &str) -> String {
        format!("{}://localhost:{}{path}", self.scheme, self.addr.port())
    }

    pub fn recorded(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }

    pub fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

async fn serve<S>(mut stream: S, handler: Handler, requests: Arc<Mutex<Vec<Recorded>>>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buf: Vec<u8> = Vec::new();
    loop {
        // Read until the end of the request head.
        let head_end = loop {
            if let Some(pos) = find(&buf, b"\r\n\r\n") {
                break pos;
            }
            let mut chunk = [0u8; 4096];
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
        let mut lines = head.split("\r\n");
        let request_line = lines.next().unwrap_or("");
        let mut parts = request_line.split(' ');
        let method = parts.next().unwrap_or("").to_owned();
        let target = parts.next().unwrap_or("").to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
            .collect();
        buf.drain(..head_end + 4);

        let get = |name: &str| {
            headers
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        let mut body = Vec::new();
        if let Some(len) = get("content-length").and_then(|v| v.parse::<usize>().ok()) {
            while buf.len() < len {
                let mut chunk = [0u8; 8192];
                match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            body = buf.drain(..len).collect();
        } else if get("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
            loop {
                let line_end = loop {
                    if let Some(p) = find(&buf, b"\r\n") {
                        break p;
                    }
                    let mut chunk = [0u8; 4096];
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                };
                let size =
                    usize::from_str_radix(String::from_utf8_lossy(&buf[..line_end]).trim(), 16)
                        .unwrap_or(0);
                buf.drain(..line_end + 2);
                while buf.len() < size + 2 {
                    let mut chunk = [0u8; 8192];
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                body.extend(buf.drain(..size));
                buf.drain(..2);
                if size == 0 {
                    break;
                }
            }
        }

        let rec = Recorded {
            method: method.clone(),
            target,
            headers,
            body,
            at: Instant::now(),
        };
        requests.lock().unwrap().push(rec.clone());
        let reply = handler(&rec);

        if reply.abort {
            return;
        }
        if !reply.delay.is_zero() {
            tokio::time::sleep(reply.delay).await;
        }
        let mut head = format!("HTTP/1.1 {} X\r\n", reply.status);
        for (n, v) in &reply.headers {
            head.push_str(&format!("{n}: {v}\r\n"));
        }
        let length = reply
            .content_length
            .clone()
            .unwrap_or_else(|| reply.body.len().to_string());
        head.push_str(&format!("Content-Length: {length}\r\n\r\n"));
        if stream.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        if method != "HEAD" {
            match reply.trickle {
                Some((piece, every)) => {
                    for part in reply.body.chunks(piece.max(1)) {
                        if stream.write_all(part).await.is_err() || stream.flush().await.is_err() {
                            return;
                        }
                        tokio::time::sleep(every).await;
                    }
                }
                None => {
                    if stream.write_all(&reply.body).await.is_err() {
                        return;
                    }
                }
            }
        }
        if stream.flush().await.is_err() || reply.close_after {
            return;
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A throw-away CA and a server certificate it signed, generated now.
pub struct TestPki {
    /// The CA certificate, DER: what a client adds to its trust anchors.
    pub ca_der: Vec<u8>,
    pub server_config: Arc<rustls::ServerConfig>,
}

impl TestPki {
    pub fn generate() -> Self {
        Self::generate_for(&[])
    }

    /// As [`TestPki::generate`], with the leaf also valid for each name in `names` (the site
    /// hosts a test reaches through `NetConfig::with_host_override`).
    pub fn generate_for(names: &[&str]) -> Self {
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        ca_params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "hatate-rs test CA (throw-away)");
        let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();

        let mut sans = vec!["localhost".to_owned()];
        sans.extend(names.iter().map(|n| (*n).to_owned()));
        let mut leaf_params = CertificateParams::new(sans).unwrap();
        leaf_params
            .subject_alt_names
            .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = leaf_params.signed_by(&leaf_key, &ca).unwrap();

        // `builder_with_provider`, not `builder`: building the server must not
        // install a process-wide default, or the fresh-process test would be moot.
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server_config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(leaf.der().to_vec())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
            )
            .unwrap();
        TestPki {
            ca_der: ca.der().to_vec(),
            server_config: Arc::new(server_config),
        }
    }
}
