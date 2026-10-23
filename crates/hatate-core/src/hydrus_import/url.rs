//! `core/hydrus_import.py`, the URL-import half (Phase 4.d, DAN-2131): handing a matched URL to
//! Hydrus's own downloader, asking first whether it can take it, following a redirect to an
//! address it can, falling back to Hatate's own download, and the bookkeeping after a confirmed
//! import. A child module of [`super`], which holds the file-upload half it builds on.
//!
//! **Errors.** As in the upload half, a call returns `Err` only for what Python lets propagate.
//! There are more of those here than there: a `ValueError` out of `urlparse` or
//! `normalize_url_for_hydrus`, a hard-deadline `HardTimeoutError` out of the redirect probe, an
//! `OSError` writing the downloaded file, and the `AttributeError`/`TypeError` of an answer of
//! the wrong shape. [`UrlImportError`] is all of them.

use std::path::Path;
use std::sync::Mutex;

use tracing::{debug, error, info, warn};

use super::{
    ImportResult, imported_bytes, is_exact_copy, joined, looks_like_a_web_page,
    record_duplicate_relationship, repoint, send_file_upload, validate_image_decodes,
    validate_magic_bytes, write_provenance_note,
};
use crate::clock::Clock;
use crate::config::Settings;
use crate::error::ErrorKind;
use crate::hydrus_client::{HydrusClient, HydrusError, dict, no_attribute};
use crate::hydrus_tag_lookup::hash_file;
use crate::hydrus_url::normalize_url_for_hydrus;
use crate::image_prep::os_error_text;
use crate::model::tags_to_send;
use crate::net::site;
use crate::net::{ErrorKind as NetErrorKind, NetClient, NetRequest, Redirects};
use crate::pycompat::{splitext, truthy};
use crate::pyrepr::py_str;
use crate::pyurl::{self, UrlValueError};
use crate::remote::{self, Raised, referer_for_candidate, secs};
use crate::search_engine::SearchEngine;

/// Hydrus's `url_type` values, from `/add_urls/get_url_info`.
pub const URL_TYPE_POST: i64 = 0;
pub const URL_TYPE_FILE: i64 = 2;
pub const URL_TYPE_GALLERY: i64 = 3;
pub const URL_TYPE_WATCHABLE: i64 = 4;
pub const URL_TYPE_UNKNOWN: i64 = 5;

/// An unknown URL is fetched by Hydrus as a raw file. That works when it IS a file, and fails
/// silently when it is a web page.
pub const MEDIA_EXTENSIONS: [&str; 15] = [
    ".jpg", ".jpeg", ".png", ".gif", ".webp", ".avif", ".bmp", ".tif", ".tiff", ".jxl", ".heic",
    ".mp4", ".webm", ".mkv", ".mov",
];

/// Seconds the one HEAD of the redirect probe is given.
pub const FORWARD_CHECK_TIMEOUT: f64 = 15.0;

/// What leaves a URL-import function as an exception.
#[derive(Debug, Clone)]
pub enum UrlImportError {
    /// A Hydrus answer of the wrong shape (`AttributeError`/`TypeError`), or a `HydrusError`
    /// Python's own handlers did not catch.
    Hydrus(HydrusError),
    /// `ValueError` from `urlparse` or `normalize_url_for_hydrus`.
    Url(UrlValueError),
    /// From `remote.download_bytes` or the redirect probe: a hard deadline, a header Latin-1
    /// cannot carry.
    Remote(Raised),
    /// `OSError` writing the downloaded file; Python's text.
    Io(String),
}

impl UrlImportError {
    /// What kind of failure this is, for a caller that decides what to show.
    pub fn kind(&self) -> ErrorKind {
        match self {
            UrlImportError::Hydrus(e) => e.kind(),
            UrlImportError::Url(_) => ErrorKind::InvalidInput,
            UrlImportError::Remote(Raised::HardTimeout(_)) => {
                ErrorKind::Net(NetErrorKind::DeadlineExceeded)
            }
            UrlImportError::Remote(_) => ErrorKind::Net(NetErrorKind::Other),
            UrlImportError::Io(_) => ErrorKind::Io,
        }
    }

    /// Python's `str(exc)`.
    pub fn message(&self) -> String {
        match self {
            UrlImportError::Hydrus(e) => e.message().to_owned(),
            UrlImportError::Url(e) => e.to_string(),
            UrlImportError::Remote(e) => e.to_string(),
            UrlImportError::Io(text) => text.clone(),
        }
    }

    /// What to tell a person: what failed and what to do, with no path, URL, query, key or OS
    /// text.
    pub fn user_message(&self) -> String {
        match self {
            UrlImportError::Hydrus(e) => e.user_message(),
            UrlImportError::Url(_) => {
                "The address of this match is not one Hatate can read, so it was not sent."
                    .to_owned()
            }
            UrlImportError::Remote(Raised::HardTimeout(_)) => {
                "The site did not finish answering in time. Try again later.".to_owned()
            }
            UrlImportError::Remote(_) => {
                "Hatate could not complete the request to the site. Try again later.".to_owned()
            }
            UrlImportError::Io(_) => {
                "Could not save the downloaded file. Check that the temporary folder exists and can be written to."
                    .to_owned()
            }
        }
    }

    /// The raw text for a log (may hold a path or an OS error).
    pub fn detail(&self) -> String {
        match self {
            UrlImportError::Hydrus(e) => e.detail(),
            other => other.message(),
        }
    }
}

impl std::fmt::Display for UrlImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for UrlImportError {}

impl From<HydrusError> for UrlImportError {
    fn from(e: HydrusError) -> Self {
        UrlImportError::Hydrus(e)
    }
}
impl From<UrlValueError> for UrlImportError {
    fn from(e: UrlValueError) -> Self {
        UrlImportError::Url(e)
    }
}
impl From<Raised> for UrlImportError {
    fn from(e: Raised) -> Self {
        UrlImportError::Remote(e)
    }
}

/// `urlparse(url)` as the parts this module reads: `(path, hostname)`.
fn path_and_host(url: &str) -> Result<(String, Option<String>), UrlValueError> {
    let parsed = pyurl::try_urlparse(url, "")?;
    Ok((parsed.path, crate::boorus::host::hostname(url)))
}

/// `value in (a, b)` for a JSON number: Python's `==`, so `3.0` is a hit and `True` is not.
fn number_is_one_of(value: Option<&serde_json::Value>, wanted: &[i64]) -> bool {
    matches!(value, Some(serde_json::Value::Number(n))
        if n.as_f64().is_some_and(|f| wanted.iter().any(|w| f == *w as f64)))
}

/// `url_import_refusal`: why Hydrus's URL importer should NOT be handed this URL, or `None`.
///
/// Two ways it goes wrong: a gallery or watchable URL imports everything there, and an unknown
/// URL that is a web page is accepted ("URL added successfully") and then fetched as a file that
/// is not one. If Hydrus cannot be asked (an older client without the endpoint, or a transient
/// error), the URL is allowed through rather than blocking every import on a failed check.
pub async fn url_import_refusal(
    client: &HydrusClient,
    url: &str,
) -> Result<Option<String>, UrlImportError> {
    let info = match client.get_url_info(url).await {
        Ok(info) => info,
        Err(e) if e.is_hydrus_error_in_python() => {
            debug!("Could not ask Hydrus about {url} ({e}) - sending it anyway");
            return Ok(None);
        }
        Err(e) => return Err(e.into()),
    };
    let info_map = dict(&info, "get")?;
    let url_type = info_map.get("url_type");
    let kind = info_map
        .get("url_type_string")
        .filter(|v| truthy(v))
        .map_or_else(|| "url".to_owned(), py_str);
    let name = info_map
        .get("match_name")
        .filter(|v| truthy(v))
        .map_or_else(|| kind.clone(), py_str);
    if number_is_one_of(url_type, &[URL_TYPE_GALLERY, URL_TYPE_WATCHABLE]) {
        return Ok(Some(format!(
            "Hydrus sees this as a {kind} ({name}) - importing it would download everything \
             there, not this one image"
        )));
    }
    if number_is_one_of(url_type, &[URL_TYPE_UNKNOWN]) {
        let (path, host) = path_and_host(url)?;
        let path = path.to_lowercase();
        if MEDIA_EXTENSIONS.iter().any(|ext| path.ends_with(ext)) {
            // A direct file link: Hydrus fetches it as one.
            return Ok(None);
        }
        let host = host
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| url.to_owned());
        return Ok(Some(format!(
            "Hydrus has no downloader for {host} - it would accept the link and then fetch the \
             web page instead of the image. Use Send (upload) or Download instead"
        )));
    }
    if number_is_one_of(url_type, &[URL_TYPE_POST])
        && matches!(
            info_map.get("can_parse"),
            Some(serde_json::Value::Bool(false))
        )
    {
        let reason = info_map
            .get("cannot_parse_reason")
            .filter(|v| truthy(v))
            .map_or_else(|| "no parser".to_owned(), py_str);
        return Ok(Some(format!(
            "Hydrus recognises this as {name} but cannot parse it ({reason})"
        )));
    }
    Ok(None)
}

/// `_forwarded_url`: where `url` finally lands after redirects, normalised for Hydrus, if that is
/// somewhere else; `None` when it does not move or can't be reached. One HEAD, and only ever
/// asked for a URL Hydrus has refused.
pub async fn forwarded_url(net: &NetClient, url: &str) -> Result<Option<String>, UrlImportError> {
    // `net.request` reads `urlparse(url).netloc` before it sends anything, and a `ValueError` is not
    // a `requests.RequestException`.
    pyurl::try_urlparse(url, "")?;
    let request =
        NetRequest::head(&pyurl::requote_uri(url), secs(FORWARD_CHECK_TIMEOUT)).map(|mut req| {
            req.headers = remote::ua_headers();
            req.with_deadline(secs(FORWARD_CHECK_TIMEOUT))
                .with_redirects(Redirects::Follow)
        });
    let response = match request {
        Ok(req) => net.send(req).await,
        Err(e) => Err(e),
    };
    let response = match response {
        Ok(response) => response,
        // `HardTimeoutError` is not a `requests.RequestException`, so it leaves the function.
        Err(e) if e.kind() == NetErrorKind::DeadlineExceeded => {
            return Err(Raised::HardTimeout(e.detail().to_string()).into());
        }
        Err(e) => {
            debug!("Could not follow {url} to see where it leads: {e}");
            return Ok(None);
        }
    };
    let landed = response.url.to_string();
    let last = normalize_url_for_hydrus(&landed)?;
    if !(last.starts_with("http://") || last.starts_with("https://"))
        || last.trim_end_matches('/') == url.trim_end_matches('/')
    {
        return Ok(None);
    }
    Ok(Some(last))
}

/// `relate_confirmed_url_import`: `record_duplicate_relationship` for an import HYDRUS'S OWN
/// downloader performed, once the poll has confirmed the file's hash.
///
/// The verdict is not re-decided here: every guard around writing it lives in
/// [`record_duplicate_relationship`] and is reached by calling it. The setting is checked on the
/// first line, before the file-id lookup and the download: with the feature off this costs no
/// request, no temp file and no comparison.
pub async fn relate_confirmed_url_import(
    entry: &crate::model::Entry,
    client: &HydrusClient,
    settings: Option<&Settings>,
    local_hash: Option<&str>,
    new_hash: &str,
) -> Result<Option<String>, HydrusError> {
    if !settings.is_some_and(|s| s.set_hydrus_duplicate_relationships) {
        return Ok(None);
    }
    let Some(local_hash) = local_hash.filter(|h| !h.is_empty()) else {
        return Ok(None);
    };
    if local_hash.to_lowercase() == new_hash.to_lowercase() {
        // Hydrus's downloader landed on the exact file the user already had: one file, no
        // relationship. Checked here too so the fetch below is not paid for a pairing that cannot
        // happen.
        return Ok(None);
    }

    let Some(downloaded) = imported_bytes(client, new_hash).await? else {
        // The user asked for relationships and did not get one, which is exactly the silence this
        // exists to remove. The import itself stands.
        return Ok(Some(
            "Imported, but not marked as a duplicate: could not read the imported file back out \
             of Hydrus to compare it against your copy"
                .to_owned(),
        ));
    };
    record_duplicate_relationship(
        entry,
        client,
        settings,
        Some(local_hash),
        &downloaded,
        new_hash,
    )
    .await
}

/// `finish_url_import`: everything that happens AFTER Hydrus's own downloader is confirmed to have
/// imported the file: the duplicate relationship and the provenance note. Each is off by default
/// and independent; both messages reach the user rather than one overwriting the other.
pub async fn finish_url_import(
    entry: &crate::model::Entry,
    client: &HydrusClient,
    settings: Option<&Settings>,
    local_hash: Option<&str>,
    new_hash: &str,
    clock: &dyn Clock,
) -> Result<Option<String>, HydrusError> {
    let relationship_warning =
        relate_confirmed_url_import(entry, client, settings, local_hash, new_hash).await?;
    let note_warning = write_provenance_note(entry, client, settings, new_hash, clock).await?;
    Ok(joined(&[
        relationship_warning.as_deref(),
        note_warning.as_deref(),
    ]))
}

/// A snapshot of the shared settings, the way Python reads the one object.
fn snapshot(settings: Option<&Mutex<Settings>>) -> Option<Settings> {
    settings.map(|m| m.lock().expect("settings lock").clone())
}

/// `send_url_or_download`: sends the URL, falling back to Hatate's own download when Hydrus can't.
///
/// Hydrus's importer first, since it brings Hydrus's own tags and keeps the source link current.
/// If Hydrus has no downloader for the site but Hatate's parser knows where the original file is,
/// Hatate downloads it and sends that. Where neither can reach the original, the refusal stands
/// and says so.
pub async fn send_url_or_download(
    entry: &mut crate::model::Entry,
    client: &HydrusClient,
    engine: &SearchEngine,
    settings: Option<&Mutex<Settings>>,
    tmp_dir: &Path,
) -> Result<ImportResult, UrlImportError> {
    let result =
        send_url_to_importer(entry, client, &engine.net, snapshot(settings).as_ref()).await?;
    if result.success || !result.refused {
        return Ok(result);
    }
    download_instead(entry, client, engine, settings, tmp_dir, &result).await
}

/// `download_instead`: the fallback half of [`send_url_or_download`]: Hatate downloads the
/// original itself, after Hydrus's importer has refused the URL.
pub async fn download_instead(
    entry: &mut crate::model::Entry,
    client: &HydrusClient,
    engine: &SearchEngine,
    settings: Option<&Mutex<Settings>>,
    tmp_dir: &Path,
    refused: &ImportResult,
) -> Result<ImportResult, UrlImportError> {
    let clock: &dyn Clock = &*engine.clock;
    if entry.selected_candidate().is_some_and(is_exact_copy) {
        // The match IS this file, byte for byte (found by the file's own SHA-256), so the local
        // copy already is the original. Uploading it gives Hydrus what a download would, and
        // works even where the site doesn't hold the original at all.
        let (_, host) = path_and_host(entry.matched_url().unwrap_or(""))?;
        let host = host
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "the site".to_owned());
        let snap = snapshot(settings);
        let mut uploaded = send_file_upload(entry, client, snap.as_ref(), clock).await?;
        if uploaded.success {
            uploaded.note = Some(format!(
                "{host} has this exact file, so Hatate sent your copy (the same bytes) with its \
                 tags and link"
            ));
            uploaded.confirmed = Some(true);
        }
        return Ok(uploaded);
    }
    let needs_details = entry
        .selected_candidate()
        .is_some_and(|c| c.direct_file_url.as_deref().is_none_or(str::is_empty));
    if needs_details && let Some(shared) = settings {
        let path = entry.path.clone();
        let index = entry.selected_candidate_index;
        if let Some(candidate) = entry.candidates.get_mut(index)
            && let Err(e) = engine
                .fetch_candidate_details(candidate, shared, None, Some(&path))
                .await
        {
            // Best effort.
            debug!("Could not look up the file for {}: {e}", entry.filename());
        }
    }
    let (_, host) = path_and_host(entry.matched_url().unwrap_or(""))?;
    let host = host
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "this site".to_owned());
    let has_direct = entry
        .selected_candidate()
        .is_some_and(|c| c.direct_file_url.as_deref().is_some_and(|u| !u.is_empty()));
    if !has_direct {
        let reason = format!(
            "Neither Hydrus nor Hatate can download the original from {host}. Use Send (upload) \
             to send your own copy with its tags and link"
        );
        warn!("Not sending {}: {reason}", entry.filename());
        entry.error_message = Some(format!("Not sent: {reason}"));
        return Ok(ImportResult {
            success: false,
            error: Some(reason),
            refused: true,
            ..ImportResult::default()
        });
    }

    info!(
        "Hydrus can't import {} - downloading it with Hatate instead",
        entry.matched_url().unwrap_or("")
    );
    let snap = snapshot(settings);
    let timeout = snap.as_ref().map_or(30.0, |s| s.search_timeout);
    let mut downloaded = download_and_send(
        entry,
        client,
        &engine.net,
        timeout,
        tmp_dir,
        snap.as_ref(),
        clock,
    )
    .await?;
    if downloaded.success {
        downloaded.note = Some(format!(
            "Hydrus has no downloader for {host}, so Hatate downloaded it"
        ));
        // Uploaded by us; Hydrus returned the hash.
        downloaded.confirmed = Some(true);
    } else if downloaded.skipped_reason.is_some() {
        downloaded = ImportResult {
            success: false,
            refused: true,
            error: refused.error.clone(),
            ..ImportResult::default()
        };
    }
    Ok(downloaded)
}

/// `send_url_to_importer`: hands the matched URL to Hydrus's own downloader; Hydrus fetches and
/// imports the file itself.
///
/// On its own, `success` only confirms that Hydrus *accepted* the request, not that the file
/// finished downloading: that needs the confirmation poll ([`crate::hydrus_import_poll`]).
/// `settings` only decides which tags ride along with the URL.
pub async fn send_url_to_importer(
    entry: &mut crate::model::Entry,
    client: &HydrusClient,
    net: &NetClient,
    settings: Option<&Settings>,
) -> Result<ImportResult, UrlImportError> {
    let Some(matched) = entry
        .matched_url()
        .filter(|u| !u.is_empty())
        .map(str::to_owned)
    else {
        return Ok(ImportResult {
            success: false,
            skipped_reason: Some("no matched URL".to_owned()),
            ..ImportResult::default()
        });
    };
    if !(matched.starts_with("http://") || matched.starts_with("https://")) {
        return Ok(ImportResult {
            success: false,
            skipped_reason: Some("matched URL isn't http(s)".to_owned()),
            ..ImportResult::default()
        });
    }

    let mut import_url = normalize_url_for_hydrus(&matched)?;

    let mut refusal = url_import_refusal(client, &import_url).await?;
    if refusal.is_some()
        && let Some(forwarded) = forwarded_url(net, &import_url).await?
        && url_import_refusal(client, &forwarded).await?.is_none()
    {
        // A legacy link that forwards to one Hydrus knows (DeviantArt's /view/<id> goes to
        // /<artist>/art/<title>-<id>). Repointed for good, so the confirmation poll and the link
        // the user opens both use the address Hydrus actually imported.
        info!("{import_url} forwards to {forwarded}, which Hydrus can import - using that");
        repoint(entry, &forwarded);
        import_url = forwarded;
        refusal = None;
    }
    if let Some(refusal) = refusal {
        // Info, not a warning: a caller may still get the file another way
        // (`send_url_or_download`), and says so itself if nothing works.
        info!(
            "Hydrus's URL importer can't take {import_url} for {}: {refusal}",
            entry.filename()
        );
        entry.error_message = Some(format!("Not sent: {refusal}"));
        return Ok(ImportResult {
            success: false,
            error: Some(refusal),
            refused: true,
            ..ImportResult::default()
        });
    }

    info!(
        "Queuing {} ({import_url}) for Hydrus's URL importer",
        entry.filename()
    );
    let tag_names: Vec<String> = tags_to_send(entry, settings)
        .iter()
        .map(|t| t.display())
        .collect();
    match client
        .import_url(&import_url, &tag_names, None, false, None)
        .await
    {
        Ok(result) => {
            let note = dict(&result, "get")?
                .get("human_result_text")
                .map_or_else(String::new, py_str);
            info!("Hydrus importer response for {}: {note}", entry.filename());
            // NOT marked confirmed: Hydrus has only accepted the URL for its downloader queue.
            entry.sent_to_hydrus = true;
            Ok(ImportResult::ok())
        }
        Err(e) if e.is_hydrus_error_in_python() => {
            entry.error_message = Some(e.message().to_owned());
            error!("import_url failed for {}: {e}", entry.filename());
            Ok(ImportResult::failed(e))
        }
        Err(e) => Err(e.into()),
    }
}

/// `download_and_send`: downloads the matched image's actual full-resolution file ourselves (not
/// the local file, not through Hydrus's downloader), then uploads those bytes to Hydrus with the
/// source URL and tags attached.
pub async fn download_and_send(
    entry: &mut crate::model::Entry,
    client: &HydrusClient,
    net: &NetClient,
    search_timeout: f64,
    tmp_dir: &Path,
    settings: Option<&Settings>,
    clock: &dyn Clock,
) -> Result<ImportResult, UrlImportError> {
    let Some(direct_url) = entry
        .selected_candidate()
        .and_then(|c| c.direct_file_url.clone())
        .filter(|u| !u.is_empty())
    else {
        return Ok(ImportResult {
            success: false,
            skipped_reason: Some("no direct file URL available".to_owned()),
            ..ImportResult::default()
        });
    };
    // The local file's own SHA-256, read before the import overwrites `entry.hydrus_hash` with the
    // NEW file's: the only hash that can still identify the copy the user already had.
    let local_hash = entry.hydrus_hash.clone();

    info!(
        "Downloading matched image for {} from {direct_url}",
        entry.filename()
    );
    let sent = download_and_send_steps(
        entry,
        client,
        net,
        search_timeout,
        tmp_dir,
        settings,
        clock,
        &direct_url,
        local_hash.as_deref(),
    )
    .await;
    match sent {
        Ok(result) => Ok(result),
        Err(UrlImportError::Hydrus(e)) if e.is_hydrus_error_in_python() => {
            entry.error_message = Some(e.message().to_owned());
            error!("Download + send failed for {}: {e}", entry.filename());
            Ok(ImportResult::failed(e))
        }
        Err(e) => Err(e),
    }
}

/// `download_and_send`'s `try` block: every `HydrusError` raised in it is caught by the caller.
#[allow(clippy::too_many_arguments)]
async fn download_and_send_steps(
    entry: &mut crate::model::Entry,
    client: &HydrusClient,
    net: &NetClient,
    search_timeout: f64,
    tmp_dir: &Path,
    settings: Option<&Settings>,
    clock: &dyn Clock,
    direct_url: &str,
    local_hash: Option<&str>,
) -> Result<ImportResult, UrlImportError> {
    let failure = |kind, message: String| UrlImportError::Hydrus(HydrusError::local(kind, message));
    let candidate = entry
        .selected_candidate()
        .cloned()
        .expect("a direct file URL came from the selected candidate");
    let referer = referer_for_candidate(&candidate, Some(direct_url));
    // Any site auth cookie too: Pixiv's CDN gates restricted works on the session, so a logged-out
    // request gets nothing.
    let cookies = settings.and_then(|s| site::cookies_for_url(direct_url, &remote::credentials(s)));
    let data = remote::download_bytes(
        net,
        Some(direct_url),
        search_timeout,
        referer.as_deref(),
        None,
        cookies,
    )
    .await?
    .filter(|d| !d.is_empty());
    let Some(data) = data else {
        return Err(failure(
            ErrorKind::DownloadFailed,
            format!("Could not download the image from {direct_url}"),
        ));
    };
    let (path, host) = path_and_host(direct_url)?;
    let host_shown = host.unwrap_or_else(|| "None".to_owned());
    if looks_like_a_web_page(&data) {
        return Err(failure(
            ErrorKind::DownloadInvalid,
            format!(
                "{host_shown} answered with a web page instead of the image - it may only serve \
                 originals to logged-in accounts"
            ),
        ));
    }

    // Integrity checks that do not depend on the remote stating a size: zero-length files,
    // implausibly small payloads, format mismatches and corrupt image data.
    let url_path_ext = splitext(&path).1.to_owned();
    let ext_for_filename = if url_path_ext.is_empty() {
        ".jpg"
    } else {
        url_path_ext.as_str()
    };
    let expected_format = candidate.remote_format.as_deref();

    if let Some(magic_error) = validate_magic_bytes(&data, expected_format, &url_path_ext) {
        return Err(failure(
            ErrorKind::DownloadInvalid,
            format!("Downloaded file validation failed: {magic_error}"),
        ));
    }

    // The size from the HEAD request, if known: catches truncated transfers, CDN corruption and
    // error pages served with 200 that don't look like web pages.
    if let Some(expected) = candidate.remote_size_bytes
        && i64::try_from(data.len()) != Ok(expected)
    {
        return Err(failure(
            ErrorKind::DownloadInvalid,
            format!(
                "Downloaded {} bytes but expected {expected} bytes (Content-Length from \
                 {host_shown}) - transfer may be truncated or corrupted",
                data.len()
            ),
        ));
    }

    if let Some(decode_error) = validate_image_decodes(&data, expected_format) {
        return Err(failure(
            ErrorKind::DownloadInvalid,
            format!("Downloaded file validation failed: {decode_error}"),
        ));
    }

    let stem = splitext(entry.filename()).0.to_owned();
    let dest = tmp_dir.join(format!("{stem}_matched{ext_for_filename}"));
    if let Err(e) = std::fs::write(&dest, &data) {
        return Err(UrlImportError::Io(os_error_text(&e, &dest)));
    }

    // The hash of what was written (defence against filesystem corruption).
    let actual_hash = hash_file(&dest);
    if actual_hash.is_empty() {
        return Err(failure(
            ErrorKind::DownloadInvalid,
            "Could not compute hash of downloaded file".to_owned(),
        ));
    }

    let result = client.import_file(&dest).await?;
    let hash_value = dict(&result, "get")?.get("hash").filter(|h| truthy(h));
    let file_hash = match hash_value {
        None => {
            return Err(failure(
                ErrorKind::ImportRejected,
                "Hydrus didn't return a file hash for this import".to_owned(),
            ));
        }
        Some(serde_json::Value::String(s)) => s.clone(),
        // `file_hash.lower()`
        Some(other) => return Err(no_attribute(other, "lower").into()),
    };

    // Hydrus's hash must be the hash of the bytes sent. On a plain `add_file` Hydrus does not
    // transcode, so a mismatch means the bytes it stored are not the bytes meant to be sent, and
    // associating a URL or tags with that hash would record success against the wrong file
    // (DAN-17). Inferred from Hydrus's documentation, not observed against a live Hydrus.
    if file_hash.to_lowercase() != actual_hash.to_lowercase() {
        return Err(failure(
            ErrorKind::HashMismatch,
            format!(
                "Hash mismatch for {} ({}): downloaded file hash={actual_hash} but Hydrus \
                 returned hash={file_hash}. Refusing to associate URL/tags with a hash that \
                 doesn't match the file we sent.",
                entry.filename(),
                dest.display()
            ),
        ));
    }

    if let Some(url) = entry
        .matched_url()
        .filter(|u| !u.is_empty())
        .map(str::to_owned)
    {
        match client.associate_url(&url, Some(&file_hash)).await {
            Ok(_) => {}
            Err(e) if e.is_hydrus_error_in_python() => {
                warn!(
                    "associate_url failed for {}: {e} (tags still attempted)",
                    entry.filename()
                );
            }
            Err(e) => return Err(e.into()),
        }
    }

    let tag_names: Vec<String> = tags_to_send(entry, settings)
        .iter()
        .map(|t| t.display())
        .collect();
    if !tag_names.is_empty() {
        client.add_tags(&file_hash, &tag_names, None).await?;
    }

    // After the file is confirmed held: Hydrus returned its hash, and that hash was checked
    // against the bytes sent. Off by default, and then it touches nothing.
    let relationship_warning =
        record_duplicate_relationship(entry, client, settings, local_hash, &data, &file_hash)
            .await?;
    let note_warning = write_provenance_note(entry, client, settings, &file_hash, clock).await?;
    let warning = joined(&[relationship_warning.as_deref(), note_warning.as_deref()]);

    entry.sent_to_hydrus = true;
    // Uploaded by us, hash returned by Hydrus.
    entry.hydrus_import_confirmed = true;
    entry.error_message.clone_from(&warning);
    entry.hydrus_hash = Some(file_hash.clone());
    info!(
        "Downloaded and sent {} to Hydrus (hash={file_hash})",
        entry.filename()
    );
    Ok(ImportResult {
        success: true,
        warning,
        ..ImportResult::default()
    })
}
