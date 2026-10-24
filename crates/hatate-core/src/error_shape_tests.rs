//! The error shape of the Phase 3 error types (DAN-2116 item 1, DAN-2216): every one answers
//! `kind()`, `user_message()` and `detail()`, and `user_message()` carries no path, URL query,
//! access key or OS text. `Display` is Python's `str(exc)`, which `Entry.error_message` keeps on
//! disk, and these tests check it did not move.

use crate::boorus::BooruError;
use crate::engine_runner::{IndexError, LensError, Raised as RunnerRaised};
use crate::engines::UnexpectedInput;
use crate::engines::ascii2d::Ascii2dError;
use crate::engines::google_images::GoogleImagesError;
use crate::engines::iqdb::IqdbError;
use crate::engines::pawchive_lookup::{PawchiveError, PawchiveLookupError};
use crate::engines::saucenao::SauceNaoError;
use crate::engines::tracemoe::TraceMoeError;
use crate::engines::yandex::YandexError;
use crate::error::ErrorKind;
use crate::hard_timeout::HardTimeoutError;
use crate::multipage_resolve::ResolveError;
use crate::pyimage::DecompressionBomb;
use crate::pyraster::OpenError;
use crate::pyurl::UrlValueError;
use crate::queue::{InvokeError, MainThreadTimeout};
use crate::remote::Raised as RemoteRaised;
use crate::search_engine::SearchError;
use crate::similarity_check::SimilarityError;

const PATH: &str = "/home/me/pictures/secret-name.png";
const URL_QUERY: &str = "https://example.test/search?api_key=SEKRET123&url=/home/me/x.png";
/// What Pillow and the OS put in a message about a file.
fn leaky() -> String {
    format!(
        "cannot identify image file '{PATH}': No such file or directory (os error 2) {URL_QUERY}"
    )
}

/// Fails if `user_message` shows anything a person should not see.
fn assert_clean(what: &str, message: &str) {
    for needle in [
        PATH,
        "secret-name",
        "/home",
        "os error",
        "SEKRET123",
        "api_key",
        "?",
        "http://",
        "https://",
        "No such file",
    ] {
        assert!(
            !message.contains(needle),
            "{what}: user_message shows {needle:?}: {message}"
        );
    }
    assert!(!message.is_empty(), "{what}: empty user_message");
    assert!(
        message.ends_with('.'),
        "{what}: a sentence that ends: {message}"
    );
}

/// One row per type: the value, its kind, and what `detail()` must still hold.
fn samples() -> Vec<(&'static str, ErrorKind, String, String)> {
    let bomb = DecompressionBomb {
        pixels: 400_000_000,
    };
    let runner = RunnerRaised(leaky());
    let remote = RemoteRaised::HardTimeout(leaky());
    let mut rows: Vec<(&'static str, ErrorKind, String, String)> = Vec::new();
    macro_rules! row {
        ($name:expr, $e:expr, $kind:expr) => {{
            let e = $e;
            rows.push(($name, $kind, e.user_message(), e.detail()));
            assert_eq!(e.kind(), $kind, "{}", $name);
        }};
    }
    row!("DecompressionBomb", bomb, ErrorKind::ImageTooLarge);
    row!(
        "OpenError::Os",
        OpenError::Os(leaky()),
        ErrorKind::ImageUndecodable
    );
    row!(
        "OpenError::Bomb",
        OpenError::Bomb(bomb),
        ErrorKind::ImageTooLarge
    );
    row!("remote::Raised::HardTimeout", remote, ErrorKind::Timeout);
    row!(
        "remote::Raised::ValueError",
        RemoteRaised::ValueError(leaky()),
        ErrorKind::InvalidUrl
    );
    row!(
        "remote::Raised::UnicodeEncode",
        RemoteRaised::UnicodeEncode(leaky()),
        ErrorKind::InvalidUrl
    );
    row!(
        "remote::Raised::BodyRead",
        RemoteRaised::BodyRead {
            class: "ProtocolError",
            message: leaky()
        },
        ErrorKind::DownloadFailed
    );
    row!(
        "engine_runner::Raised",
        runner.clone(),
        ErrorKind::Unexpected
    );
    row!("IndexError", IndexError(leaky()), ErrorKind::Database);
    row!("UrlValueError", UrlValueError, ErrorKind::InvalidUrl);
    row!(
        "HardTimeoutError",
        HardTimeoutError(leaky()),
        ErrorKind::Timeout
    );
    row!(
        "ResolveError::Raised",
        ResolveError::Raised(RemoteRaised::ValueError(leaky())),
        ErrorKind::InvalidUrl
    );
    row!(
        "ResolveError::Bomb",
        ResolveError::Bomb(bomb),
        ErrorKind::ImageTooLarge
    );
    row!(
        "ResolveError::Url",
        ResolveError::Url(UrlValueError),
        ErrorKind::InvalidUrl
    );
    row!(
        "SimilarityError::Raised",
        SimilarityError::Raised(RemoteRaised::HardTimeout(leaky())),
        ErrorKind::Timeout
    );
    row!(
        "SimilarityError::Bomb",
        SimilarityError::Bomb(bomb),
        ErrorKind::ImageTooLarge
    );
    row!(
        "SearchError::Engine",
        SearchError::Engine(runner),
        ErrorKind::Unexpected
    );
    row!(
        "SearchError::Remote",
        SearchError::Remote(RemoteRaised::BodyRead {
            class: "DecodeError",
            message: leaky()
        }),
        ErrorKind::DownloadFailed
    );
    row!(
        "SearchError::Resolve",
        SearchError::Resolve(ResolveError::Url(UrlValueError)),
        ErrorKind::InvalidUrl
    );
    row!(
        "SearchError::Similarity",
        SearchError::Similarity(SimilarityError::Bomb(bomb)),
        ErrorKind::ImageTooLarge
    );
    row!(
        "SearchError::Bomb",
        SearchError::Bomb(bomb),
        ErrorKind::ImageTooLarge
    );
    row!("IqdbError", IqdbError(leaky()), ErrorKind::EngineFailed);
    row!(
        "SauceNaoError",
        SauceNaoError(leaky()),
        ErrorKind::EngineFailed
    );
    row!(
        "TraceMoeError",
        TraceMoeError(leaky()),
        ErrorKind::EngineFailed
    );
    row!(
        "PawchiveLookupError",
        PawchiveLookupError(leaky()),
        ErrorKind::EngineFailed
    );
    row!(
        "PawchiveError::Lookup",
        PawchiveError::Lookup(PawchiveLookupError(leaky())),
        ErrorKind::EngineFailed
    );
    row!(
        "PawchiveError::HardTimeout",
        PawchiveError::HardTimeout(HardTimeoutError(leaky())),
        ErrorKind::Timeout
    );
    row!(
        "Ascii2dError::Failed",
        Ascii2dError::Failed(leaky()),
        ErrorKind::EngineFailed
    );
    row!(
        "Ascii2dError::Blocked",
        Ascii2dError::Blocked,
        ErrorKind::EngineBlocked
    );
    row!(
        "GoogleImagesError::Failed",
        GoogleImagesError::Failed(leaky()),
        ErrorKind::EngineFailed
    );
    row!(
        "GoogleImagesError::Blocked",
        GoogleImagesError::Blocked(leaky()),
        ErrorKind::EngineBlocked
    );
    row!(
        "GoogleImagesError::JavaScript",
        GoogleImagesError::JavaScript,
        ErrorKind::EngineBlocked
    );
    row!(
        "GoogleImagesError::Unexpected",
        GoogleImagesError::Unexpected(UnexpectedInput),
        ErrorKind::Unexpected
    );
    row!(
        "YandexError::Failed",
        YandexError::Failed(leaky()),
        ErrorKind::EngineFailed
    );
    row!(
        "YandexError::Blocked",
        YandexError::Blocked(leaky()),
        ErrorKind::EngineBlocked
    );
    row!(
        "YandexError::Unexpected",
        YandexError::Unexpected(UnexpectedInput),
        ErrorKind::Unexpected
    );
    row!("UnexpectedInput", UnexpectedInput, ErrorKind::Unexpected);
    row!(
        "BooruError::Fetch",
        BooruError::Fetch(leaky()),
        ErrorKind::DownloadFailed
    );
    row!(
        "BooruError::ContentGone",
        BooruError::ContentGone(leaky()),
        ErrorKind::NotFound
    );
    // The types that already had the split before this ticket: the same bar.
    row!(
        "LensError::Unavailable",
        LensError::Unavailable(leaky()),
        ErrorKind::EngineUnavailable
    );
    row!(
        "LensError::Blocked",
        LensError::Blocked(leaky()),
        ErrorKind::EngineBlocked
    );
    row!(
        "LensError::Failed",
        LensError::Failed(leaky()),
        ErrorKind::EngineFailed
    );
    row!(
        "LensError::Unexpected",
        LensError::Unexpected(leaky()),
        ErrorKind::Unexpected
    );
    row!(
        "InvokeError::Timeout",
        InvokeError::Timeout(MainThreadTimeout(leaky())),
        ErrorKind::McpQueueTimeout
    );
    row!(
        "InvokeError::Failed",
        InvokeError::Failed(leaky()),
        ErrorKind::McpQueueFailed
    );
    rows
}

#[test]
fn no_user_message_shows_a_path_a_query_a_key_or_os_text() {
    let rows = samples();
    assert!(
        rows.len() >= 45,
        "a type was dropped from the table: {}",
        rows.len()
    );
    for (name, _, message, _) in &rows {
        assert_clean(name, message);
    }
}

/// The variants that hold no text of their own: their `detail()` is a fixed description.
const NO_RAW_TEXT: [&str; 12] = [
    "DecompressionBomb",
    "OpenError::Bomb",
    "UrlValueError",
    "ResolveError::Bomb",
    "ResolveError::Url",
    "SimilarityError::Bomb",
    "SearchError::Resolve",
    "SearchError::Similarity",
    "SearchError::Bomb",
    "Ascii2dError::Blocked",
    "GoogleImagesError::JavaScript",
    "UnexpectedInput",
];

#[test]
fn detail_keeps_the_raw_text_for_the_log() {
    for (name, _, _, detail) in samples() {
        if NO_RAW_TEXT.contains(&name)
            || name.ends_with("::Unexpected") && !name.starts_with("Lens")
        {
            assert!(!detail.is_empty(), "{name}: empty detail");
        } else {
            assert!(
                detail.contains("os error 2"),
                "{name}: detail lost the raw text: {detail}"
            );
        }
    }
}

#[test]
fn a_queue_timeout_has_the_shape_too() {
    // The 5.e verifier's P2 note: the `Timeout` variant of `InvokeError`, which maps to
    // `ErrorKind::McpQueueTimeout`, was the one the error-shape test did not cover.
    let e = InvokeError::Timeout(MainThreadTimeout(format!(
        "timed out waiting on {PATH}: os error 110"
    )));
    assert_eq!(e.kind(), ErrorKind::McpQueueTimeout);
    assert_clean("InvokeError::Timeout", &e.user_message());
    assert!(e.detail().contains("os error 110"));
    // Python's `str(exc)`, unchanged.
    assert_eq!(
        e.to_string(),
        format!("timed out waiting on {PATH}: os error 110")
    );
}

#[test]
fn display_is_still_pythons_text() {
    // `Entry.error_message` is written from `Display`; it must not have moved.
    assert_eq!(
        IqdbError("IQDB request failed".into()).to_string(),
        "IQDB request failed"
    );
    assert_eq!(
        SearchError::Remote(RemoteRaised::ValueError("bad".into())).to_string(),
        "bad"
    );
    assert_eq!(UrlValueError.to_string(), "urlparse raised ValueError");
    assert_eq!(
        OpenError::Os("cannot identify image file 'x'".into()).to_string(),
        "cannot identify image file 'x'"
    );
}

#[test]
fn a_missing_file_open_names_neither_the_path_nor_the_os() {
    // DAN-2116 item 1: a missing-file open yields a user_message with neither.
    let missing = std::path::Path::new("/nonexistent-dir-hatate/never-there.png");
    let io = std::fs::read(missing).unwrap_err();
    let open = OpenError::Os(format!("{}: {io}", missing.display()));
    let message = open.user_message();
    assert!(!message.contains("never-there") && !message.contains("nonexistent"));
    assert!(!message.contains("os error") && !message.contains("No such file"));
    assert!(open.detail().contains("os error 2"));
}
