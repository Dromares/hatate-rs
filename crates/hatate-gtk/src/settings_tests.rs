//! The Settings dialog's Test buttons (`gui/settings_dialog.py` at `76c85c4`: `_test_deviantart`
//! `:1294`, `_test_animepictures` `:1384`, `_test_sankaku` `:1478`, `_test_e621` `:1685`,
//! `_test_pixiv` `:1763`, `_test_hydrus` `:1882`) as plain functions.
//!
//! Each Test has two halves with the Qt `processEvents()` between them. [`plan`] is everything
//! that happens before the request: the URL check, reading the typed cookies and the first line
//! of the result label. It touches no network and returns either the final text ([`Plan::Done`])
//! or the request to send ([`Plan::Run`]). [`run`] sends it and turns the answer into the final
//! text. The dialog (`settings_dialog.rs`) shows the first line, runs the second half on the
//! worker bridge, and puts the answer in the label.
//!
//! The text of a failure is not Python's `str(exc)`: a message that can carry an address or an OS
//! error is replaced by one that says what failed and what to do (DAN-2116 item 1; D-G of
//! `docs/gtk-6h2.md`). Nothing here puts a cookie, a key or a token in a text or a log.

use hatate_core::boorus::fetch::{BooruSession, FetchOptions, fetch_page_info};
use hatate_core::boorus::{BooruError, BooruPageInfo};
use hatate_core::net::site::{
    ANIMEPICTURES_SESSION_COOKIE, SiteCredentials, animepictures_cookies, cookie_paste_warnings,
    headers_for_url, parse_cookie_string,
};
use hatate_core::net::{Cookies, NetClient, NetRequest, RetryMode, Secret};
use std::time::Duration;

/// The cookie that carries a Sankaku login: `cookie_paste_warnings`' default `session_name`.
const SANKAKU_SESSION_COOKIE: &str = "_sankakuchannel_session";

/// What `check_url_available(..., timeout=20.0)` and `requests.get(..., timeout=20)` wait.
pub const FIXED_TIMEOUT: f64 = 20.0;

/// Which button was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Test {
    DeviantArt,
    AnimePictures,
    Sankaku,
    E621,
    Pixiv,
}

impl Test {
    /// The id of the button (`button.<this>` in `settings_spec.rs`).
    pub fn button_id(self) -> &'static str {
        match self {
            Test::DeviantArt => "button.test_deviantart",
            Test::AnimePictures => "button.test_anime_pictures",
            Test::Sankaku => "button.test_sankaku",
            Test::E621 => "button.test_e621_login",
            Test::Pixiv => "button.test_pixiv",
        }
    }

    /// The id of the result label.
    pub fn result_id(self) -> &'static str {
        match self {
            Test::DeviantArt => "deviantart_test_result",
            Test::AnimePictures => "animepictures_test_result",
            Test::Sankaku => "sankaku_test_result",
            Test::E621 => "e621_test_result",
            Test::Pixiv => "pixiv_test_result",
        }
    }

    /// The id of the cookie box the Test reads (Pixiv's is one cookie, e621's is a pair).
    pub fn cookie_id(self) -> Option<&'static str> {
        match self {
            Test::DeviantArt => Some("deviantart_cookies"),
            Test::AnimePictures => Some("animepictures_cookies"),
            Test::Sankaku => Some("sankaku_cookies"),
            Test::Pixiv => Some("pixiv_session_cookie"),
            Test::E621 => None,
        }
    }

    /// The question `QInputDialog.getText` asks first: `(title, label, preset text)`. e621 asks
    /// nothing.
    pub fn question(self) -> Option<(&'static str, &'static str, &'static str)> {
        match self {
            Test::DeviantArt => Some((
                "Test DeviantArt",
                "Paste a DeviantArt deviation URL (ideally an adult one,\nsince that is what a login actually changes):",
                "https://www.deviantart.com/",
            )),
            Test::AnimePictures => Some((
                "Test Anime-Pictures",
                "Paste an Anime-Pictures post URL (ideally an account-only one):",
                "https://anime-pictures.net/pictures/view_post/",
            )),
            Test::Sankaku => Some((
                "Test Sankaku",
                "Paste a Sankaku post URL (ideally one you can only see when logged in):",
                "",
            )),
            Test::Pixiv => Some((
                "Test Pixiv",
                "Paste any Pixiv artwork URL (either the modern /artworks/ form\nor a legacy member_illust.php one):",
                "https://www.pixiv.net/artworks/",
            )),
            Test::E621 => None,
        }
    }
}

/// What the fields held when the answer to the question came in (Python reads them after it).
#[derive(Debug, Clone, Default)]
pub struct Typed {
    /// The URL from the question box, or `""` for e621.
    pub url: String,
    /// The Test's cookie box (Pixiv's single cookie), or `""`.
    pub cookies: String,
    pub e621_username: String,
    pub e621_api_key: String,
    /// `self.settings.search_timeout`: the saved value, not a typed one.
    pub search_timeout: f64,
}

/// The request half of a Test.
#[derive(Clone)]
pub enum Job {
    /// `fetch_page_info(url, timeout, cookies, headers)` and then the site's reading of it.
    Page {
        site: Test,
        url: String,
        cookies: Option<Cookies>,
        headers: Option<hatate_core::net::HeaderMap>,
        has_cookies: bool,
        timeout: f64,
    },
    /// `check_url_available(url, timeout=20.0, cookies)`.
    Available {
        url: String,
        cookies: Option<Cookies>,
        timeout: f64,
    },
    /// `requests.get("https://e621.net/posts.json?limit=1", headers, timeout=20)`.
    E621 {
        user: String,
        key: String,
        timeout: f64,
    },
}

/// What a Test does before its request.
pub enum Plan {
    /// Nothing to send: this is the label's text.
    Done(String),
    /// Show `first`, then send `job`. `first` is `None` where Python sets no text before it.
    Run { first: Option<String>, job: Job },
}

/// `Testing with N cookie(s): a, b…` and the paste warnings.
fn testing_with(cookies: &Cookies, warnings: &[String]) -> String {
    let mut names: Vec<&str> = cookies.names().collect();
    names.sort_unstable();
    let mut msg = format!(
        "Testing with {} cookie(s): {}\u{2026}",
        cookies.len(),
        names.join(", ")
    );
    if !warnings.is_empty() {
        msg.push_str("\n\n\u{26a0} ");
        msg.push_str(&warnings.join("\n\u{26a0} "));
    }
    msg
}

const NO_COOKIES: &str =
    "No cookies entered - testing anonymously, which is what the app would do.";

/// The part of a Test before its request.
pub fn plan(test: Test, typed: &Typed) -> Plan {
    let url = typed.url.trim().to_owned();
    let timeout = typed.search_timeout;
    match test {
        Test::DeviantArt => {
            if !url.contains("deviantart.com") && !url.contains("fav.me") {
                return Plan::Done("That does not look like a DeviantArt URL.".into());
            }
            let cookies = parse_cookie_string(&typed.cookies);
            if cookies.is_empty() && !typed.cookies.trim().is_empty() {
                return Plan::Done(
                    "That doesn't look like cookies. They need to be name=value pairs separated by semicolons, e.g. auth=\u{2026}; auth_secure=\u{2026}"
                        .into(),
                );
            }
            let first = if cookies.is_empty() {
                NO_COOKIES.to_owned()
            } else {
                testing_with(&cookies, &cookie_paste_warnings(&typed.cookies, "auth"))
            };
            Plan::Run {
                first: Some(first),
                job: Job::Page {
                    site: test,
                    url,
                    has_cookies: !cookies.is_empty(),
                    cookies: scoped(cookies, ["deviantart.com", "fav.me"]),
                    headers: None,
                    timeout,
                },
            }
        }
        Test::AnimePictures => {
            if !url.contains("anime-pictures.net") {
                return Plan::Done("That does not look like an Anime-Pictures URL.".into());
            }
            let cookies = animepictures_cookies(&typed.cookies).unwrap_or_default();
            if cookies.is_empty() && !typed.cookies.trim().is_empty() {
                return Plan::Done(format!(
                    "That doesn't look like a cookie. Paste it as {ANIMEPICTURES_SESSION_COOKIE}=<value>, or paste just the value on its own - but it has to be one unbroken run of text, so check nothing was pasted with a line break in the middle."
                ));
            }
            let first = if cookies.is_empty() {
                NO_COOKIES.to_owned()
            } else {
                testing_with(
                    &cookies,
                    &cookie_paste_warnings(&typed.cookies, ANIMEPICTURES_SESSION_COOKIE),
                )
            };
            Plan::Run {
                first: Some(first),
                job: Job::Page {
                    site: test,
                    url,
                    has_cookies: !cookies.is_empty(),
                    cookies: scoped(cookies, ["anime-pictures.net"]),
                    headers: None,
                    timeout,
                },
            }
        }
        Test::Sankaku => {
            // `if not ok or not url.strip(): return` is the dialog's; there is no URL check here
            let cookies = parse_cookie_string(&typed.cookies);
            let first = if cookies.is_empty() {
                NO_COOKIES.to_owned()
            } else {
                testing_with(
                    &cookies,
                    &cookie_paste_warnings(&typed.cookies, SANKAKU_SESSION_COOKIE),
                )
            };
            Plan::Run {
                first: Some(first),
                job: Job::Available {
                    url,
                    cookies: scoped(cookies, ["sankakucomplex.com"]),
                    timeout: FIXED_TIMEOUT,
                },
            }
        }
        Test::E621 => {
            let user = typed.e621_username.trim().to_owned();
            let key = typed.e621_api_key.trim().to_owned();
            if user.is_empty() || key.is_empty() {
                let missing = if user.is_empty() {
                    "username"
                } else {
                    "API key"
                };
                return Plan::Done(format!(
                    "No {missing} entered. Both halves are needed - with one missing the app sends no credentials at all and e621 answers as if you were logged out."
                ));
            }
            Plan::Run {
                first: Some(format!("Testing as {user}\u{2026}")),
                job: Job::E621 {
                    user,
                    key,
                    timeout: FIXED_TIMEOUT,
                },
            }
        }
        Test::Pixiv => {
            if !url.contains("pixiv.net") {
                return Plan::Done("That does not look like a Pixiv URL.".into());
            }
            let cookie = typed.cookies.trim();
            let cookies = (!cookie.is_empty()).then(|| {
                let mut c = Cookies::new().with_scope(["pixiv.net"]);
                c.set("PHPSESSID", cookie);
                c
            });
            Plan::Run {
                first: Some("Testing\u{2026}".into()),
                job: Job::Page {
                    site: test,
                    headers: headers_for_url(&url, None),
                    has_cookies: cookies.is_some(),
                    cookies,
                    url,
                    timeout,
                },
            }
        }
    }
}

/// The cookies, scoped to the site's own domains (so they follow its redirects and go nowhere
/// else), or none for an empty jar: `cookies=cookies or None`.
fn scoped<const N: usize>(cookies: Cookies, domains: [&str; N]) -> Option<Cookies> {
    (!cookies.is_empty()).then(|| cookies.with_scope(domains))
}

/// The request half: sends it and returns the label's final text.
pub async fn run(job: Job, net: &NetClient, session: &BooruSession) -> String {
    match job {
        Job::Page {
            site,
            url,
            cookies,
            headers,
            has_cookies,
            timeout,
        } => {
            let opts = FetchOptions {
                timeout,
                cookies,
                headers,
                ..FetchOptions::default()
            };
            match fetch_page_info(net, session, &url, opts).await {
                Ok(info) => read_page(site, &info, has_cookies),
                Err(e) => failed_fetch(site, &e),
            }
        }
        Job::Available {
            url,
            cookies,
            timeout,
        } => {
            match hatate_core::availability::check_url_available(
                net, Some(&url), timeout, None, None, cookies, None,
            )
            .await
            {
                Ok(Some(true)) => {
                    "The post loaded normally - it would be kept as a live match.".into()
                }
                Ok(Some(false)) => "Came back as the blank \"no such post\" page, so it would be dropped. If you can see this post in your browser while logged in, the cookies above aren't working - re-copy them.".into(),
                Ok(None) | Err(_) => "Couldn't tell (network error, timeout, or a block). The match would be kept rather than dropped.".into(),
            }
        }
        Job::E621 { user, key, timeout } => run_e621(&user, &key, timeout, net).await,
    }
}

/// `requests.get(url, headers=headers_for_url(url, typed), timeout=20)`: one try, no retry.
async fn run_e621(user: &str, key: &str, timeout: f64, net: &NetClient) -> String {
    let url = "https://e621.net/posts.json?limit=1";
    let creds = SiteCredentials {
        e621_username: Secret::new(user),
        e621_api_key: Secret::new(key),
        ..SiteCredentials::default()
    };
    let unreachable = || {
        "Couldn't reach e621. That is a network problem, not a verdict on the credentials. See Help > View Logs for the specific reason.".to_owned()
    };
    let Ok(mut req) = NetRequest::get(url, Duration::from_secs_f64(timeout)) else {
        return unreachable();
    };
    if let Some(headers) = headers_for_url(url, Some(&creds)) {
        req.headers = headers;
    }
    let req = req.with_retry(RetryMode::Never);
    let status = match net.send(req).await {
        Ok(resp) => resp.status.as_u16(),
        Err(e) => {
            tracing::warn!("e621 test: no answer ({:?})", e.kind());
            return unreachable();
        }
    };
    match status {
        200 => format!(
            "e621 accepted the credentials for {user}. Posts under its global blacklist will be visible to searches."
        ),
        401 => "e621 rejected the username or API key (401). Check both on e621 under Account > Manage API Access - the key is not your password, and it is case-sensitive.\n\nLeave them blank rather than wrong: a bad key makes e621 refuse every request, including ones that would have worked anonymously.".into(),
        403 => "e621 answered 403. The credentials may be right but the account is blocked from the API, or the request was refused for another reason.".into(),
        503 => "e621 answered 503 - it is rate-limiting or under load. Try again in a moment; this says nothing about the credentials.".into(),
        other => format!(
            "e621 answered HTTP {other}, which is neither an accept nor a rejection. Try again shortly."
        ),
    }
}

fn artist_of(info: &BooruPageInfo, namespace: &str) -> Option<String> {
    info.tags
        .iter()
        .find(|t| t.namespace.as_deref() == Some(namespace))
        .map(|t| t.name.clone())
}

fn has(text: &Option<String>) -> bool {
    text.as_deref().is_some_and(|s| !s.is_empty())
}

/// What a fetched page means for one site's Test.
fn read_page(site: Test, info: &BooruPageInfo, has_cookies: bool) -> String {
    match site {
        Test::DeviantArt => {
            let artist = artist_of(info, "artist");
            if has(&info.restricted) {
                tracing::info!("DeviantArt test: still blurred");
                return format!(
                    "Read the deviation{}, but the picture is still the BLURRED logged-out copy. {}",
                    artist
                        .as_ref()
                        .map(|a| format!(" (artist '{a}')"))
                        .unwrap_or_default(),
                    if has_cookies {
                        "Re-copy the cookies from a logged-in browser - they may have expired or been cut off."
                    } else {
                        "Add cookies from a logged-in account above and retry."
                    }
                );
            }
            if artist.is_none() && !has(&info.preview_url) {
                return "Nothing could be read from that URL. DeviantArt's data endpoint only accepts the full /artist/art/title-12345 form - a /view/ or fav.me link is resolved automatically, but only if the site answers. See Help > View Logs.".into();
            }
            let mut parts = vec!["Working.".to_owned()];
            if let Some(a) = &artist {
                parts.push(format!("Artist '{a}'."));
            }
            parts.push(
                if has(&info.preview_url) {
                    "Picture available (unblurred)."
                } else {
                    "No picture came back."
                }
                .to_owned(),
            );
            let mut summary = parts.join(" ");
            if !has_cookies {
                summary
                    .push_str(" (No cookies set - adult deviations will still come back blurred.)");
            }
            tracing::info!("DeviantArt test succeeded");
            summary
        }
        Test::AnimePictures => {
            if has(&info.restricted) {
                tracing::info!("Anime-Pictures test: still restricted");
                return format!(
                    "Still account-only with these cookies. {}",
                    if has_cookies {
                        "Re-copy them from a logged-in browser - they may have expired."
                    } else {
                        "Add cookies from a logged-in account above and retry."
                    }
                );
            }
            if info.tags.is_empty() && info.width.unwrap_or(0) == 0 {
                return "The post was fetched but nothing could be read from it. See Help > View Logs for the specific reason.".into();
            }
            let mut parts = vec![format!("Working. {} tag(s)", info.tags.len())];
            if let (Some(w), Some(h)) = (
                info.width.filter(|w| *w != 0),
                info.height.filter(|h| *h != 0),
            ) {
                parts.push(format!("{w}\u{d7}{h}"));
            }
            if let Some(f) = info.file_format.as_deref().filter(|f| !f.is_empty()) {
                parts.push(f.to_owned());
            }
            let mut summary = parts.join(", ") + ".";
            if !has_cookies {
                summary
                    .push_str(" (No cookies set - account-only posts will still come back blank.)");
            }
            tracing::info!("Anime-Pictures test succeeded");
            summary
        }
        Test::Pixiv => {
            let tag_count = info.tags.len();
            let has_file = has(&info.file_url);
            if tag_count > 0 || has_file {
                let mut parts = vec![format!("Working. Extracted {tag_count} tag(s)")];
                if let Some(c) = artist_of(info, "creator") {
                    parts.push(format!("artist '{c}'"));
                }
                parts.push(
                    if has_file {
                        "full-resolution URL found"
                    } else {
                        "but NO full-res URL"
                    }
                    .to_owned(),
                );
                if let (Some(w), Some(h)) = (
                    info.width.filter(|w| *w != 0),
                    info.height.filter(|h| *h != 0),
                ) {
                    parts.push(format!("{w}x{h}"));
                }
                let mut summary = parts.join(", ") + ".";
                if !has_cookies {
                    summary.push_str(" (No cookie set - restricted works will still fail.)");
                }
                tracing::info!("Pixiv test succeeded");
                return summary;
            }
            tracing::warn!("Pixiv test returned no usable data");
            if has_cookies {
                "Page fetched, but no data could be read from it. The cookie may have expired (they do, periodically) - try copying a fresh PHPSESSID from your browser. See Help > View Logs for the specific reason.".into()
            } else {
                "Page fetched, but no data could be read from it. If this artwork requires login or is R-18, set a session cookie above and retry. See Help > View Logs for the specific reason.".into()
            }
        }
        Test::Sankaku | Test::E621 => unreachable!("these Tests do not read a page"),
    }
}

/// `except BooruContentGoneError` and `except BooruError`. The second is not Python's
/// `f"Could not fetch it: {exc}"`: the fetch's text carries the address (D-G).
fn failed_fetch(site: Test, err: &BooruError) -> String {
    tracing::warn!("{site:?} test: {}", err.detail());
    match (err, site) {
        (BooruError::ContentGone(_), Test::DeviantArt) => "That deviation is gone (HTTP 404) - deleted, or the URL is wrong. Try another; this says nothing about whether the cookies work.".into(),
        (BooruError::ContentGone(_), Test::AnimePictures) => "That post is gone (HTTP 404) - it was deleted or the ID does not exist. Try a different one; this says nothing about whether the cookies work.".into(),
        (BooruError::ContentGone(_), _) => "That artwork is gone (HTTP 404) - it was deleted or the ID does not exist. Try a different URL; this does not tell us whether the cookie works.".into(),
        (BooruError::Fetch(_), _) => "Could not fetch it: the site did not answer as expected. Check the address and your connection, then try again. See Help > View Logs for the specific reason.".into(),
    }
}

// -- Hydrus --------------------------------------------------------------------------------------

/// `_test_hydrus`: the label's text for what the client answered.
pub fn hydrus_text(answer: &Result<bool, hatate_core::hydrus_client::HydrusError>) -> String {
    match answer {
        Ok(true) => "Connected \u{2713}".into(),
        Ok(false) => "No response".into(),
        Err(e) => {
            tracing::warn!("Hydrus connection test failed: {}", e.detail());
            format!("Failed: {}", e.user_message())
        }
    }
}

/// An item of the fetched-services list: `f"{name}  ({key[:12]}…)"`.
pub fn service_label(key: &str, name: &str) -> String {
    let head: String = key.chars().take(12).collect();
    format!("{name}  ({head}\u{2026})")
}
