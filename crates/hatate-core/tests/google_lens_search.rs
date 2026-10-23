//! `google_lens.search` and its state over a scripted browser (DAN-2163): the failure counter and
//! its latch, the challenge rest and its end, the unavailable alert, the pacing sleeps, the format
//! check that refuses without latching, and the connection state a UI reads.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use hatate_core::clock::{BoxFuture, Clock, ManualClock};
use hatate_core::engine_alerts::{EngineAlert, EngineAlerts};
use hatate_core::engine_runner::LensError;
use hatate_core::engines::lens_search::{
    CHALLENGE_REST_SECONDS, LensClient, LensConnection, LensFetch, LensFetchError, LensMissing,
    LensPayloads, MAX_CONSECUTIVE_FAILURES, MIN_SECONDS_BETWEEN_SEARCHES,
};
use hatate_core::error::ErrorKind;
use serde_json::{Value, json};

/// What the scripted browser was asked, and what it answers next.
#[derive(Default)]
struct Script {
    answers: Mutex<std::collections::VecDeque<Result<LensPayloads, LensFetchError>>>,
    asked: Mutex<Vec<(usize, String, f64, bool)>>,
}

impl Script {
    fn then(&self, answer: Result<LensPayloads, LensFetchError>) -> &Self {
        self.answers.lock().unwrap().push_back(answer);
        self
    }

    fn asked(&self) -> Vec<(usize, String, f64, bool)> {
        self.asked.lock().unwrap().clone()
    }
}

struct Shared(Arc<Script>);

impl LensFetch for Shared {
    fn fetch_results_payloads<'a>(
        &'a self,
        image: &'a [u8],
        filename: &'a str,
        timeout: f64,
        whole_image: bool,
    ) -> BoxFuture<'a, Result<LensPayloads, LensFetchError>> {
        self.0
            .asked
            .lock()
            .unwrap()
            .push((image.len(), filename.to_owned(), timeout, whole_image));
        let answer = self.0.answers.lock().unwrap().pop_front();
        Box::pin(std::future::ready(answer.expect(
            "the scripted browser was asked more often than it was scripted for",
        )))
    }
}

struct Rig {
    client: LensClient,
    script: Arc<Script>,
    clock: Arc<ManualClock>,
    alerts: Arc<EngineAlerts>,
    raised: Arc<Mutex<Vec<EngineAlert>>>,
    connections: Arc<Mutex<Vec<LensConnection>>>,
    dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let script = Arc::new(Script::default());
    let clock = Arc::new(ManualClock::new(1_000_000.0, 500.0));
    let alerts = Arc::new(EngineAlerts::new());
    let raised = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&raised);
    alerts.subscribe(Arc::new(move |a: &EngineAlert| {
        sink.lock().unwrap().push(a.clone())
    }));
    let connections = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&connections);
    let client = LensClient::with_fetch(Arc::new(Shared(Arc::clone(&script))))
        .with_clock(Arc::clone(&clock) as Arc<dyn Clock>)
        .with_alerts(Arc::clone(&alerts))
        .with_connection_sink(Arc::new(move |c| seen.lock().unwrap().push(c)));
    Rig {
        client,
        script,
        clock,
        alerts,
        raised,
        connections,
        dir: tempfile::Builder::new()
            .prefix("hatate-dan2163-")
            .tempdir()
            .unwrap(),
    }
}

impl Rig {
    fn png(&self, name: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        let mut out = Vec::new();
        image::RgbImage::from_pixel(6, 4, image::Rgb([20, 80, 200]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&path, out).unwrap();
        path
    }

    async fn search(
        &self,
        path: &Path,
    ) -> Result<Vec<hatate_core::engines::google_lens::GoogleLensMatch>, LensError> {
        self.client.search(path, 30.0, None, true).await
    }
}

fn page(tiles: Value) -> LensPayloads {
    LensPayloads {
        payloads: vec!["<html>a page</html>".to_owned()],
        tiles,
    }
}

fn browser_failed() -> Result<LensPayloads, LensFetchError> {
    Err(LensFetchError::Browser("page crashed".to_owned()))
}

#[tokio::test]
async fn a_search_ranks_exact_matches_by_position_and_reports_ready() {
    let rig = rig();
    rig.script.then(Ok(page(json!([
        {"text": "Post 5674224: Ben_10 Gwen - Rule 34 Paheal 800x600", "thumb": ""},
        {"text": "Rule 34 | 4585855 / - Rule 34 1,072x1,108", "thumb": ""},
    ]))));
    let path = rig.png("a.png");
    let found = rig.search(&path).await.unwrap();
    let rows: Vec<_> = found
        .iter()
        .map(|m| (m.url.as_str(), m.similarity, m.width, m.height))
        .collect();
    assert_eq!(
        rows,
        vec![
            // The Paheal pattern takes the rest of the line as the title, so no size follows it.
            (
                "https://rule34.paheal.net/post/view/5674224",
                80.0,
                None,
                None
            ),
            (
                "https://rule34.xxx/index.php?page=post&s=view&id=4585855",
                78.0,
                Some(1072),
                Some(1108)
            ),
        ]
    );
    // The browser was handed the picture under its own name, the floor of the budget, and the
    // whole-image flag.
    let asked = rig.script.asked();
    assert_eq!(
        (asked[0].1.as_str(), asked[0].2, asked[0].3),
        ("a.png", 90.0, true)
    );
    assert_eq!(
        *rig.connections.lock().unwrap(),
        vec![LensConnection::Ready]
    );
    assert_eq!(rig.client.connection(), Some(LensConnection::Ready));
    assert!(!rig.client.is_blocked());
}

#[tokio::test]
async fn three_failed_images_in_a_row_stand_the_engine_down_until_a_search_starts_again() {
    let rig = rig();
    for _ in 0..MAX_CONSECUTIVE_FAILURES {
        rig.script.then(browser_failed());
    }
    let path = rig.png("a.png");
    let first = rig.search(&path).await.unwrap_err();
    assert_eq!(
        first,
        LensError::Failed("the browser could not read the results (page crashed)".to_owned())
    );
    assert!(!rig.client.is_blocked());
    assert!(matches!(rig.search(&path).await, Err(LensError::Failed(_))));
    let third = rig.search(&path).await.unwrap_err();
    let LensError::Blocked(text) = &third else {
        panic!("{third:?}")
    };
    assert!(text.starts_with("the browser could not read the results (page crashed) That has now happened to 3 images in a row,"));
    assert!(rig.client.is_blocked());
    assert!(!rig.client.is_resting(), "a stand-down is not a rest");
    assert_eq!(
        *rig.connections.lock().unwrap(),
        vec![LensConnection::Blocked]
    );

    rig.client.reset_blocked_flag();
    assert!(!rig.client.is_blocked());
    // The count starts again: one failure is just a failure.
    rig.script.then(browser_failed());
    assert!(matches!(rig.search(&path).await, Err(LensError::Failed(_))));
}

#[tokio::test]
async fn a_good_page_clears_the_run_of_failures() {
    let rig = rig();
    let path = rig.png("a.png");
    rig.script.then(browser_failed()).then(browser_failed());
    rig.script.then(Ok(page(json!([]))));
    rig.script.then(browser_failed()).then(browser_failed());
    for _ in 0..2 {
        assert!(matches!(rig.search(&path).await, Err(LensError::Failed(_))));
    }
    assert_eq!(rig.search(&path).await.unwrap(), vec![]);
    for _ in 0..2 {
        assert!(matches!(rig.search(&path).await, Err(LensError::Failed(_))));
    }
    assert!(!rig.client.is_blocked());
}

#[tokio::test]
async fn a_page_with_nothing_in_it_counts_as_a_failure() {
    let rig = rig();
    let path = rig.png("a.png");
    rig.script.then(Ok(LensPayloads {
        payloads: vec![],
        tiles: json!([]),
    }));
    let err = rig.search(&path).await.unwrap_err();
    assert_eq!(
        err,
        LensError::Failed(
            "Google returned a results page with nothing in it. If the browser window showed \
             \"can't read file\", this image is one Google will not accept."
                .to_owned()
        )
    );
}

#[tokio::test]
async fn an_unanswered_robot_check_rests_for_thirty_minutes_and_survives_a_new_search() {
    let rig = rig();
    let path = rig.png("a.png");
    rig.script.then(Err(LensFetchError::ChallengeUnanswered(
        "window closed".to_owned(),
    )));
    let err = rig.search(&path).await.unwrap_err();
    // 1_000_000 s since the epoch is 13:46 UTC; the rest ends 1800 s later.
    let until = rig.client.resting_until();
    assert_eq!(until, "14:16");
    let LensError::Blocked(text) = &err else {
        panic!("{err:?}")
    };
    assert!(text.contains(&format!(
        "so Lens is resting until {until} - asking again straight away"
    )));
    assert!(rig.client.is_resting() && rig.client.is_blocked());

    // Starting a search does not end it: that is the point of a rest.
    rig.client.reset_blocked_flag();
    assert!(rig.client.is_blocked());

    rig.clock.advance(CHALLENGE_REST_SECONDS - 1.0);
    assert!(rig.client.is_resting());
    rig.clock.advance(2.0);
    assert!(!rig.client.is_resting() && !rig.client.is_blocked());
    assert_eq!(rig.client.resting_until(), "");

    rig.script
        .then(Err(LensFetchError::ChallengeUnanswered("again".to_owned())));
    assert!(rig.search(&path).await.is_err());
    assert!(rig.client.is_resting());
    rig.client.end_rest();
    assert!(!rig.client.is_blocked());
    assert_eq!(rig.client.connection(), Some(LensConnection::Blocked));
}

#[tokio::test]
async fn an_unavailable_browser_latches_and_tells_the_user_once() {
    let rig = rig();
    let path = rig.png("a.png");
    for _ in 0..2 {
        rig.script.then(Err(LensFetchError::Unavailable {
            message: "pip install playwright".to_owned(),
            missing: Some(LensMissing::Playwright),
        }));
    }
    let err = rig.search(&path).await.unwrap_err();
    assert_eq!(
        err,
        LensError::Unavailable("pip install playwright".to_owned())
    );
    assert!(rig.client.is_blocked());
    rig.client.reset_blocked_flag();
    assert!(matches!(
        rig.search(&path).await,
        Err(LensError::Unavailable(_))
    ));
    // Two searches, one alert: the key names the condition, not the image.
    let raised = rig.raised.lock().unwrap();
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].key, "google-lens:playwright-not-installed");
    assert_eq!(raised[0].title, "Google Lens cannot run");
    assert_eq!(raised[0].remedy, "pip install playwright");
    assert!(
        raised[0]
            .body
            .starts_with("Google Lens was skipped: the Playwright package it needs")
    );
    assert!(
        rig.alerts
            .already_raised("google-lens:playwright-not-installed")
    );
}

#[tokio::test]
async fn the_alert_text_follows_what_is_missing() {
    let rig = rig();
    let cases = [
        (
            Some(LensMissing::Chromium),
            "google-lens:chromium-not-installed",
            "Playwright is installed but the Chromium",
        ),
        (
            None,
            "google-lens:browser-unavailable",
            "its browser could not be started.\n\nno luck",
        ),
    ];
    for (missing, key, body) in cases {
        assert!(rig.client.announce_unavailable("no luck", missing));
        let alert = rig.raised.lock().unwrap().last().cloned().unwrap();
        assert_eq!(alert.key, key);
        assert!(alert.body.contains(body), "{}", alert.body);
        assert_eq!(alert.remedy.is_empty(), missing.is_none());
    }
    let bundled = rig.client.clone().running_as_appimage(true);
    let fresh = EngineAlerts::new();
    let bundled = bundled.with_alerts(Arc::new(fresh));
    assert!(bundled.announce_unavailable("x", Some(LensMissing::Chromium)));
}

#[tokio::test]
async fn a_video_is_refused_with_a_reason_and_does_not_latch_or_count() {
    let rig = rig();
    let video = rig.dir.path().join("clip.mp4");
    std::fs::write(&video, b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00mp42isom").unwrap();
    for _ in 0..(MAX_CONSECUTIVE_FAILURES + 1) {
        let err = rig.search(&video).await.unwrap_err();
        assert_eq!(
            err,
            LensError::Failed(
                "clip.mp4 is not in a format Google Lens accepts, and it could not be converted \
                 to one. Video and JPEG XL are the usual reasons; the other engines can still \
                 search it."
                    .to_owned()
            )
        );
    }
    assert!(!rig.client.is_blocked());
    assert!(
        rig.script.asked().is_empty(),
        "the browser is never asked about a refused file"
    );
    assert!(rig.connections.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_gif_is_converted_to_a_jpeg_under_a_jpg_name_before_it_goes_up() {
    let rig = rig();
    let path = rig.dir.path().join("pic.gif");
    let mut out = Vec::new();
    image::RgbaImage::from_pixel(5, 5, image::Rgba([10, 200, 10, 255]))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Gif)
        .unwrap();
    std::fs::write(&path, out).unwrap();
    rig.script.then(Ok(page(json!([]))));
    rig.search(&path).await.unwrap();
    assert_eq!(rig.script.asked()[0].1, "pic.jpg");
}

#[tokio::test]
async fn an_unreadable_file_is_a_failed_image_whose_user_message_names_no_path() {
    let rig = rig();
    let missing = rig.dir.path().join("gone.png");
    let err = rig.search(&missing).await.unwrap_err();
    let LensError::Failed(text) = &err else {
        panic!("{err:?}")
    };
    assert!(text.starts_with("Could not read image: [Errno 2] No such file or directory: '"));
    assert!(text.contains("gone.png"));
    // The side channel: a kind, a message a person can act on, and the raw text.
    assert_eq!(err.kind(), ErrorKind::EngineFailed);
    let message = err.user_message();
    assert!(!message.contains("gone.png") && !message.contains(rig.dir.path().to_str().unwrap()));
    assert!(!message.contains("os error") && !message.contains("Errno"));
    assert_eq!(err.detail(), *text);
    assert!(!rig.client.is_blocked() && rig.script.asked().is_empty());
}

#[test]
fn every_error_has_a_kind_and_a_message_with_no_url_key_or_path_in_it() {
    let secret = "https://www.google.com/search?tbs=sbi:AMhZZ&access_key=abc123 /home/me/.config/lens_profile";
    let cases = [
        (
            LensError::Unavailable(secret.to_owned()),
            ErrorKind::EngineUnavailable,
        ),
        (
            LensError::Blocked(secret.to_owned()),
            ErrorKind::EngineBlocked,
        ),
        (
            LensError::Failed(secret.to_owned()),
            ErrorKind::EngineFailed,
        ),
        (
            LensError::Unexpected(secret.to_owned()),
            ErrorKind::Unexpected,
        ),
    ];
    for (err, kind) in cases {
        assert_eq!(err.kind(), kind);
        let message = err.user_message();
        for leak in [
            "http",
            "?",
            "access_key",
            "abc123",
            "/home",
            "lens_profile",
            "os error",
        ] {
            assert!(!message.contains(leak), "{message:?} leaks {leak:?}");
        }
        assert_eq!(err.detail(), secret);
    }
}

#[tokio::test]
async fn the_second_search_waits_out_the_45_seconds_in_one_second_steps() {
    let rig = rig();
    rig.client.wait_for_turn(None, None).await;
    assert!(rig.clock.sleeps().is_empty());
    let ticks = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&ticks);
    rig.client
        .wait_for_turn(
            Some(Arc::new(move |label: &str, left: f64, total: f64| {
                seen.lock().unwrap().push((label.to_owned(), left, total));
            })),
            None,
        )
        .await;
    let sleeps = rig.clock.sleeps();
    assert_eq!(sleeps.len(), MIN_SECONDS_BETWEEN_SEARCHES as usize);
    assert!(sleeps.iter().all(|s| *s == 1.0));
    {
        let ticks = ticks.lock().unwrap();
        assert_eq!(
            ticks.first().unwrap(),
            &("Pacing Google Lens".to_owned(), 45.0, 45.0)
        );
        assert_eq!(
            ticks.last().unwrap(),
            &("Pacing Google Lens".to_owned(), 1.0, 45.0)
        );
    }

    // A third arriving at once is spaced a turn after the second, not let go with it.
    let before = rig.clock.sleeps().len();
    rig.client.wait_for_turn(None, None).await;
    assert_eq!(rig.clock.sleeps().len() - before, 45);
}

#[tokio::test]
async fn pacing_stops_when_asked_to_and_a_rest_ending_clears_the_turn() {
    let rig = rig();
    rig.client.wait_for_turn(None, None).await;
    let stop: hatate_core::engines::saucenao::ShouldStop = Arc::new(|| true);
    rig.client.wait_for_turn(None, Some(stop)).await;
    assert!(rig.clock.sleeps().is_empty());
    rig.client.end_rest();
    rig.client.wait_for_turn(None, None).await;
    assert!(
        rig.clock.sleeps().is_empty(),
        "end_rest forgets the claimed turn"
    );
}

#[tokio::test]
async fn a_picture_over_the_upload_budget_is_re_encoded_before_it_goes_up() {
    let rig = rig();
    // Noise does not compress: 1200x1200 RGB is over 2 MB as a PNG.
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let raw: Vec<u8> = (0..1200 * 1200 * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect();
    let path = rig.dir.path().join("big.png");
    image::RgbImage::from_raw(1200, 1200, raw)
        .unwrap()
        .save(&path)
        .unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > 2 * 1024 * 1024);
    rig.script.then(Ok(page(json!([]))));
    rig.search(&path).await.unwrap();
    let (len, name, _, _) = rig.script.asked().remove(0);
    assert_eq!(name, "big.jpg");
    assert!((len as u64) <= 2 * 1024 * 1024);
}
