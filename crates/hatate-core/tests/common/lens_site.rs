//! A fake Lens site for the browser tests: a tiny HTTP server on 127.0.0.1 that serves a
//! SafeSearch page with radios, ordinary search pages, a robot check that stays until the test
//! clears it, and a request log. Nothing is captured from Google; the pages are synthetic.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// What the site has seen and what state it is in.
#[derive(Default)]
pub struct SiteState {
    /// `METHOD path?query` for every request, in order.
    pub log: Mutex<Vec<String>>,
    /// While set, `/search` answers with a page carrying a reCAPTCHA iframe.
    pub challenged: AtomicBool,
}

pub struct Site {
    pub base: String,
    pub state: Arc<SiteState>,
}

const SAFESEARCH: &str = r#"<!doctype html><title>SafeSearch</title>
<div role="radio" id="filter" aria-checked="false" style="width:200px;height:40px;margin:10px">Filter
more text</div>
<div role="radio" id="blur" aria-checked="true" style="width:200px;height:40px;margin:10px">Blur</div>
<div role="radio" id="off" aria-checked="false" style="width:200px;height:40px;margin:10px">Off</div>
<script>
for (const r of document.querySelectorAll('[role=radio]')) {
  r.addEventListener('click', (e) => {
    fetch('/_click?id=' + r.id + '&x=' + e.clientX + '&y=' + e.clientY);
  });
}
</script>"#;

const PLAIN: &str = "<!doctype html><title>results</title><p>results";

const CAPTCHA: &str = "<!doctype html><title>check</title><p>check \
    <iframe src=\"/recaptcha/api2/anchor\" title=\"reCAPTCHA\"></iframe>";

impl Site {
    pub async fn start() -> Site {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(SiteState::default());
        let served = Arc::clone(&state);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let state = Arc::clone(&served);
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let mut parts = head.lines().next().unwrap_or("").split(' ');
                    let (method, target) =
                        (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
                    let path = target.split('?').next().unwrap_or("/");
                    if !target.starts_with("/favicon") {
                        state.log.lock().unwrap().push(format!("{method} {target}"));
                    }
                    let (status, body) = match path {
                        "/safesearch" => (200, SAFESEARCH),
                        "/search" if state.challenged.load(Ordering::SeqCst) => (200, CAPTCHA),
                        "/search" => (200, PLAIN),
                        "/recaptcha/api2/anchor" => (200, PLAIN),
                        "/_click" | "/" => (200, PLAIN),
                        _ => (404, "not found"),
                    };
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Site { base, state }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn log(&self) -> Vec<String> {
        self.state.log.lock().unwrap().clone()
    }

    pub fn set_challenged(&self, on: bool) {
        self.state.challenged.store(on, Ordering::SeqCst);
    }
}
