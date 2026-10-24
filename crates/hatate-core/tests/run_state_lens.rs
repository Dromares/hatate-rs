//! `LensConnection` on the `RunState` channel (DAN-2216): the same sink that carries the run's
//! states carries the Lens client's, through `LensClient::with_connection_sink`, in order. Each of
//! the four connection states is reached by a real search over a scripted browser.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hatate_core::clock::{BoxFuture, Clock, ManualClock};
use hatate_core::engine_runner::LensError;
use hatate_core::engines::lens_search::{
    LensClient, LensConnection, LensFetch, LensFetchError, LensMissing, LensPayloads,
    MAX_CONSECUTIVE_FAILURES,
};
use hatate_core::run_state::{RunChannel, RunEvent, RunSink};
use serde_json::json;

#[derive(Default)]
struct Script {
    answers: Mutex<VecDeque<Result<LensPayloads, LensFetchError>>>,
}

struct Shared(Arc<Script>);

impl LensFetch for Shared {
    fn fetch_results_payloads<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: f64,
        _: bool,
    ) -> BoxFuture<'a, Result<LensPayloads, LensFetchError>> {
        let answer = self.0.answers.lock().unwrap().pop_front();
        Box::pin(std::future::ready(
            answer.expect("asked more often than scripted"),
        ))
    }
}

struct Rig {
    client: LensClient,
    script: Arc<Script>,
    clock: Arc<ManualClock>,
    seen: Arc<Mutex<Vec<RunEvent>>>,
    dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let script = Arc::new(Script::default());
    let clock = Arc::new(ManualClock::new(1_000_000.0, 500.0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink: RunSink = {
        let seen = Arc::clone(&seen);
        Arc::new(move |e| seen.lock().unwrap().push(e))
    };
    let channel = RunChannel::new(sink, clock.clone() as Arc<dyn Clock>);
    let client = LensClient::with_fetch(Arc::new(Shared(Arc::clone(&script))))
        .with_clock(clock.clone() as Arc<dyn Clock>)
        .with_connection_sink(channel.lens_sink());
    Rig {
        client,
        script,
        clock,
        seen,
        dir: tempfile::Builder::new()
            .prefix("hatate-dan2216-")
            .tempdir()
            .unwrap(),
    }
}

impl Rig {
    fn png(&self) -> PathBuf {
        let path = self.dir.path().join("a.png");
        let mut out = Vec::new();
        image::RgbImage::from_pixel(6, 4, image::Rgb([20, 80, 200]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&path, out).unwrap();
        path
    }

    fn then(&self, answer: Result<LensPayloads, LensFetchError>) {
        self.script.answers.lock().unwrap().push_back(answer);
    }

    fn heard(&self) -> Vec<LensConnection> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|e| match e {
                RunEvent::Lens(c) => *c,
                other => panic!("only Lens changes come from the client: {other:?}"),
            })
            .collect()
    }
}

fn page() -> LensPayloads {
    LensPayloads {
        payloads: vec!["<html>a page</html>".to_owned()],
        tiles: json!([{"text": "Post 5674224: Ben_10 Gwen - Rule 34 Paheal 800x600", "thumb": ""}]),
    }
}

#[tokio::test]
async fn ready_follows_waiting_for_lens_on_the_run_channel() {
    let rig = rig();
    rig.then(Ok(page()));
    rig.client
        .search(&rig.png(), 30.0, None, true)
        .await
        .unwrap();
    assert_eq!(
        rig.heard(),
        [LensConnection::WaitingForLens, LensConnection::Ready]
    );
}

#[tokio::test]
async fn a_browser_that_cannot_start_is_not_reachable_on_the_channel() {
    let rig = rig();
    rig.then(Err(LensFetchError::Unavailable {
        message: "no browser".to_owned(),
        missing: Some(LensMissing::Chromium),
    }));
    assert!(matches!(
        rig.client.search(&rig.png(), 30.0, None, true).await,
        Err(LensError::Unavailable(_))
    ));
    assert_eq!(
        rig.heard(),
        [LensConnection::WaitingForLens, LensConnection::NotReachable]
    );
}

#[tokio::test]
async fn an_unanswered_robot_check_and_a_run_of_failures_are_blocked_on_the_channel() {
    let rig = rig();
    rig.then(Err(LensFetchError::ChallengeUnanswered(
        "captcha".to_owned(),
    )));
    assert!(matches!(
        rig.client.search(&rig.png(), 30.0, None, true).await,
        Err(LensError::Blocked(_))
    ));
    assert_eq!(
        rig.heard(),
        [LensConnection::WaitingForLens, LensConnection::Blocked]
    );

    // And the other road to Blocked: the stand-down after several failures in a row.
    let rig = rig();
    let path = rig.png();
    for _ in 0..MAX_CONSECUTIVE_FAILURES {
        rig.then(Err(LensFetchError::Browser("page crashed".to_owned())));
        rig.clock.advance(1000.0);
        let _ = rig.client.search(&path, 30.0, None, true).await;
    }
    assert_eq!(rig.heard().last(), Some(&LensConnection::Blocked));
}
