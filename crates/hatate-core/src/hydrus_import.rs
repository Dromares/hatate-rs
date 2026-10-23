//! `core/hydrus_import.py`, the file-upload half (Phase 4.c, DAN-2130): sending one local file to
//! Hydrus, the validators a downloaded file goes through, the duplicate verdict and the two pieces
//! of bookkeeping that follow an import (the duplicate relationship and the provenance note).
//!
//! The URL-import half (`url_import_refusal`, `send_url_or_download`, `download_and_send`, ...) is
//! Phase 4.d; `normalize_url_for_hydrus` is [`crate::hydrus_url`].
//!
//! **Errors.** Python's functions catch `HydrusError` and turn it into a result or a warning
//! string; anything else propagates. A call here returns `Err` only for what Python would let
//! propagate ([`ErrorKind::UnexpectedResponse`]: an answer whose body is not the shape the code
//! reads). A caught `HydrusError` is also kept on [`ImportResult::fault`], so a caller can show
//! [`HydrusError::user_message`] (no path, URL query, key or OS text) while `error` stays the
//! byte-compatible string `Entry.error_message` carries.

use std::path::{Path, PathBuf};

use tracing::{debug, error, info, warn};

use crate::clock::Clock;
use crate::config::Settings;
use crate::engine_ids::{PAWCHIVE, label_for};
use crate::error::ErrorKind;
use crate::hydrus_client::{
    DUPLICATE_ALTERNATE, DUPLICATE_BETTER, DeletionState, HydrusClient, HydrusError,
    PERMISSION_EDIT_FILE_NOTES, PERMISSION_EDIT_FILE_RELATIONSHIPS,
};
use crate::image_compare::{
    DHASH_SAME_MAX, DHASH_SIMILAR_MAX, FINE_CONFIRM_MAX, Source, compare_to_local, local_prints,
};
use crate::model::{Candidate, Entry, tag_version_for_url, tags_to_send};
use crate::provenance_note::{note_name, provenance_note};
use crate::pycompat::truthy;
use crate::pyraster;

/// `ImportResult`: what one import attempt came to.
#[derive(Debug, Clone, Default)]
pub struct ImportResult {
    pub success: bool,
    /// Partial success: imported, but the URL or the tags failed.
    pub warning: Option<String>,
    /// Complete failure.
    pub error: Option<String>,
    /// E.g. no matched URL, or no direct file URL available.
    pub skipped_reason: Option<String>,
    /// Hydrus was asked about the URL and can't import it (as opposed to a failed request).
    pub refused: bool,
    /// What was done instead, for the status line.
    pub note: Option<String>,
    /// `None`: not applicable to this method. `Some(true)`: Hydrus confirmed the import finished.
    /// `Some(false)`: it never confirmed in time.
    pub confirmed: Option<bool>,
    /// The typed error behind `error`, for the caller that shows
    /// [`HydrusError::user_message`]. Not in Python; never stored.
    pub fault: Option<HydrusError>,
}

impl PartialEq for ImportResult {
    /// The Python dataclass's fields; `fault` is the side channel for the message and is not
    /// compared.
    fn eq(&self, other: &Self) -> bool {
        (
            self.success,
            &self.warning,
            &self.error,
            &self.skipped_reason,
            self.refused,
            &self.note,
            self.confirmed,
        ) == (
            other.success,
            &other.warning,
            &other.error,
            &other.skipped_reason,
            other.refused,
            &other.note,
            other.confirmed,
        )
    }
}

impl ImportResult {
    pub fn ok() -> Self {
        ImportResult {
            success: true,
            ..Self::default()
        }
    }

    fn failed(error: HydrusError) -> Self {
        ImportResult {
            success: false,
            error: Some(error.message().to_owned()),
            fault: Some(error),
            ..Self::default()
        }
    }
}

/// `send_file_upload`: uploads the local file, associates the matched URL, adds tags.
///
/// `settings` is read for which tags go with the file ([`tags_to_send`]) and for
/// `write_hydrus_provenance_note`. `None` sends every tag on the entry and writes no note.
///
/// Always uploads, even when `entry.hydrus_hash` is already set: that hash is the file's SHA-256,
/// computed for every added file during duplicate detection, and says nothing about whether Hydrus
/// has the file. `add_file` is idempotent and reports "already in database" with the hash.
pub async fn send_file_upload(
    entry: &mut Entry,
    client: &HydrusClient,
    settings: Option<&Settings>,
    clock: &dyn Clock,
) -> Result<ImportResult, HydrusError> {
    info!("Sending {} to Hydrus (upload)", entry.filename());
    let imported = client.import_file(Path::new(&entry.path)).await;
    let outcome = match imported {
        Ok(result) => file_hash_of(&result, entry.hydrus_hash.as_deref()).and_then(|hash| {
            entry.hydrus_hash.clone_from(&hash);
            info!(
                "Imported {} -> hash={} status={}",
                entry.filename(),
                hash.as_deref().unwrap_or("None"),
                result
                    .get("status")
                    .map_or_else(|| "None".to_owned(), crate::pyrepr::py_str)
            );
            match hash.filter(|h| !h.is_empty()) {
                Some(h) => Ok(h),
                None => Err(HydrusError::local(
                    ErrorKind::ImportRejected,
                    "Hydrus didn't return a file hash for this import",
                )),
            }
        }),
        Err(e) => Err(e),
    };
    let file_hash = match outcome {
        Ok(hash) => hash,
        Err(e) if e.is_hydrus_error_in_python() => {
            entry.error_message = Some(e.message().to_owned());
            error!("Failed to import {}: {e}", entry.filename());
            return Ok(ImportResult::failed(e));
        }
        Err(e) => return Err(e),
    };

    // The file is in Hydrus at this point: the URL and the tags are attempted independently so
    // one failing doesn't throw away the successful import.
    let mut step_warnings: Vec<String> = Vec::new();
    if let Some(url) = entry.matched_url().filter(|u| !u.is_empty()).map(str::to_owned) {
        match client.associate_url(&url, Some(&file_hash)).await {
            Ok(_) => {}
            Err(e) if e.is_hydrus_error_in_python() => {
                step_warnings.push(format!("URL not added: {e}"));
                error!("add_url failed for {}: {e}", entry.filename());
            }
            Err(e) => return Err(e),
        }
    }

    let tag_names: Vec<String> = tags_to_send(entry, settings)
        .iter()
        .map(|t| t.display())
        .collect();
    if !tag_names.is_empty() {
        match client.add_tags(&file_hash, &tag_names, None).await {
            Ok(_) => {}
            Err(e) if e.is_hydrus_error_in_python() => {
                step_warnings.push(format!("Tags not added: {e}"));
                error!("add_tags failed for {}: {e}", entry.filename());
            }
            Err(e) => return Err(e),
        }
    }

    // After the file is confirmed held. Off by default, and then it makes no request.
    if let Some(warning) = write_provenance_note(entry, client, settings, &file_hash, clock).await? {
        step_warnings.push(warning);
    }

    entry.sent_to_hydrus = true;
    // Hydrus returned a hash for the file itself.
    entry.hydrus_import_confirmed = true;
    if step_warnings.is_empty() {
        entry.error_message = None;
        return Ok(ImportResult::ok());
    }
    let joined = step_warnings.join("; ");
    entry.error_message = Some(joined.clone());
    Ok(ImportResult {
        success: true,
        warning: Some(joined),
        ..ImportResult::default()
    })
}

/// `result.get("hash") or entry.hydrus_hash`. Python carries a hash that is not a string along;
/// this does not (class C2 of `docs/hydrus-import.md`).
fn file_hash_of(
    result: &serde_json::Value,
    current: Option<&str>,
) -> Result<Option<String>, HydrusError> {
    match result.get("hash") {
        Some(v) if truthy(v) => match v {
            serde_json::Value::String(s) => Ok(Some(s.clone())),
            other => Err(HydrusError::local(
                ErrorKind::UnexpectedResponse,
                format!(
                    "Hydrus returned a {} for the file hash, not a string",
                    crate::hydrus_client::type_name(other)
                ),
            )),
        },
        _ => Ok(current.map(str::to_owned)),
    }
}

/// `_repoint`: the URL that goes to Hydrus replaces the stored one, on the entry and on the
/// candidate it was read from, so the table, the export and the session all show what Hydrus was
/// actually given.
pub fn repoint(entry: &mut Entry, url: &str) {
    let current = entry.matched_url().map(str::to_owned);
    match entry.candidates.get_mut(entry.selected_candidate_index) {
        Some(candidate) => {
            if current.as_deref() == Some(candidate.url.as_str()) {
                url.clone_into(&mut candidate.url);
                candidate.tag_version = tag_version_for_url(url);
            }
        }
        // No selected match: `matched_url` is the one left behind.
        None => entry.stale_matched_url = Some(url.to_owned()),
    }
}

/// `_is_exact_copy`: whether a match was found by the file's own hash, so it IS the same file
/// (currently the Pawchive lookup).
pub fn is_exact_copy(candidate: &Candidate) -> bool {
    candidate.engine == label_for(PAWCHIVE)
}

/// `_looks_like_a_web_page`: a login wall or hotlink block answers 200 with HTML. Hydrus would
/// reject it anyway, but only after the upload, with a vaguer reason.
pub fn looks_like_a_web_page(data: &[u8]) -> bool {
    let head = &data[..data.len().min(512)];
    let start = head
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
        .unwrap_or(head.len());
    let head = head[start..].to_ascii_lowercase();
    [&b"<!doctype html"[..], b"<html", b"<head", b"<?xml"]
        .iter()
        .any(|p| head.starts_with(p))
}

fn is_webp(data: &[u8]) -> bool {
    data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"WEBP"
}

fn is_isobmff(data: &[u8]) -> bool {
    data.len() >= 12
        && &data[4..8] == b"ftyp"
        && matches!(
            &data[8..12],
            b"avif" | b"heic" | b"mif1" | b"msf1" | b"mp4 " | b"isom" | b"iso2"
        )
}

fn is_ebml(data: &[u8]) -> bool {
    data.starts_with(b"\x1aE\xdf\xa3")
}

/// `_MAGIC_BYTES[format]`, applied: whether `data` carries one of the format's signatures. `None`
/// for a format the table does not know.
fn matches_signature(format: &str, data: &[u8]) -> Option<bool> {
    Some(match format {
        "JPEG" => data.starts_with(b"\xFF\xD8\xFF"),
        "PNG" => data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "GIF" => data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"),
        "WEBP" => is_webp(data),
        "BMP" => data.starts_with(b"BM"),
        "TIFF" => data.starts_with(b"II\x2a\x00") || data.starts_with(b"MM\x00\x2a"),
        "AVIF" | "HEIC" | "MP4" => is_isobmff(data),
        "WEBM" | "MKV" => is_ebml(data),
        _ => return None,
    })
}

const KNOWN_FORMATS: [&str; 11] = [
    "JPEG", "PNG", "GIF", "WEBP", "BMP", "TIFF", "AVIF", "HEIC", "MP4", "WEBM", "MKV",
];

/// The format a file extension names (`jpg`, `.PNG`, ...), when it names one.
fn format_from_ext(file_ext: &str) -> Option<&'static str> {
    let lowered = file_ext.to_lowercase();
    Some(match lowered.trim_start_matches('.') {
        "jpg" | "jpeg" => "JPEG",
        "png" => "PNG",
        "gif" => "GIF",
        "webp" => "WEBP",
        "bmp" => "BMP",
        "tif" | "tiff" => "TIFF",
        "avif" => "AVIF",
        "heic" => "HEIC",
        "mp4" => "MP4",
        "webm" => "WEBM",
        "mkv" => "MKV",
        _ => return None,
    })
}

/// `_validate_magic_bytes`: whether the downloaded data's magic bytes match the expected format.
///
/// An error message if validation fails; `None` if it passes or the format is unknown (an unknown
/// format is not failed closed). With no `expected_format` the extension names one if it can;
/// with neither, the payload is sniffed against every known signature and rejected if none fits.
pub fn validate_magic_bytes(
    data: &[u8],
    expected_format: Option<&str>,
    file_ext: &str,
) -> Option<String> {
    if data.is_empty() {
        return Some("Downloaded file is empty (0 bytes)".to_owned());
    }
    // A real image is rarely under 100 bytes.
    if data.len() < 100 {
        return Some(format!(
            "Downloaded file is implausibly small ({} bytes)",
            data.len()
        ));
    }

    let mut expected = expected_format.filter(|f| !f.is_empty()).map(str::to_owned);
    if expected.is_none() {
        expected = format_from_ext(file_ext).map(str::to_owned);
    }

    if let Some(expected) = expected {
        let expected = expected.to_uppercase();
        return match matches_signature(&expected, data) {
            // Unknown format: don't fail closed.
            None => {
                debug!("No magic byte signatures known for format {expected}; skipping validation");
                None
            }
            Some(true) => None,
            Some(false) => Some(format!(
                "Magic bytes do not match expected format {expected}"
            )),
        };
    }

    for format in KNOWN_FORMATS {
        if matches_signature(format, data) == Some(true) {
            debug!("Sniffed format {format} from magic bytes (no expected format)");
            return None;
        }
    }
    Some(
        "Downloaded file does not match any known image/video format (magic bytes unrecognized)"
            .to_owned(),
    )
}

/// The formats Pillow decodes here (video formats are skipped).
const PIL_FORMATS: [&str; 7] = ["JPEG", "PNG", "GIF", "WEBP", "BMP", "TIFF", "AVIF"];

/// What `repr(io.BytesIO(...))` shows in Pillow's "cannot identify image file" text. Python prints
/// the object's address, which differs from run to run; this is a fixed one of the same shape
/// (class C3 of `docs/hydrus-import.md`).
const BYTESIO_REPR: &str = "<_io.BytesIO object at 0x7f0000000000>";

/// `_validate_image_decodes`: tries to decode the data to confirm it is a valid image.
///
/// An error message if decoding fails; `None` if it succeeds or if the format is not one Pillow
/// handles (video). With no `expected_format` the magic bytes have already said it is a known
/// format, and a video format would fail a decode, so nothing is tried.
pub fn validate_image_decodes(data: &[u8], expected_format: Option<&str>) -> Option<String> {
    let expected = expected_format.filter(|f| !f.is_empty())?;
    let upper = expected.to_uppercase();
    if !PIL_FORMATS.contains(&upper.as_str()) {
        return None;
    }
    // This build has no AVIF decoder (class C4): a file that says it is AVIF is not decoded, which
    // is what a format Python does not decode gets too, and the magic bytes were already checked.
    if is_isobmff(data) && &data[8..12] == b"avif" {
        return None;
    }
    match pyraster::open(data, None) {
        Ok(_) => None,
        Err(pyraster::OpenError::Os(text)) => {
            let text = if text == "cannot identify image file" {
                format!("{text} {BYTESIO_REPR}")
            } else {
                text
            };
            Some(format!(
                "Downloaded data does not decode as a valid image: {text}"
            ))
        }
        Err(pyraster::OpenError::Bomb(bomb)) => Some(format!(
            "Downloaded data does not decode as a valid image: {bomb}"
        )),
    }
}

/// What the two files turned out to be, when that can be said at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// One picture, two copies: the new one is better.
    SameImage,
    /// One picture, two versions: a different edit or crop.
    Alternate,
}

impl Verdict {
    /// `VERDICT_SAME_IMAGE` / `VERDICT_ALTERNATE`.
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::SameImage => "same",
            Verdict::Alternate => "alternate",
        }
    }
}

/// `duplicate_verdict`: whether the file just downloaded is the same picture as the local one, a
/// different edit of it, or something not to guess at.
///
/// Measured here, from the two real files, rather than read off `candidate.similarity`, which is a
/// comparison against the engine's thumbnail and flattens several outcomes onto one number. `None`
/// means "do not touch Hydrus": no distance at all, the mirrored shape, no 256-bit distance, a
/// 64-bit "same" that 256 bits does not confirm, or a distance past `DHASH_SIMILAR_MAX`.
pub fn duplicate_verdict(local_path: &Path, downloaded: &[u8]) -> Option<Verdict> {
    let comparison = compare_to_local(
        local_prints(Source::Path(local_path)).as_ref(),
        Source::Bytes(downloaded),
    );
    let (distance, fine) = match (comparison.distance, comparison.fine_distance) {
        (Some(d), Some(f)) if !comparison.mirrored => (d, f),
        _ => return None,
    };
    if distance <= DHASH_SAME_MAX {
        return (fine <= FINE_CONFIRM_MAX).then_some(Verdict::SameImage);
    }
    if distance <= DHASH_SIMILAR_MAX {
        return Some(Verdict::Alternate);
    }
    None
}

/// `record_duplicate_relationship`: tells Hydrus how the file just imported relates to the user's
/// local copy, so its library does not end up holding two files it believes are strangers.
///
/// Off unless the user asked for it, and then it returns before doing anything at all. Never fails
/// the import: a problem worth telling the user about comes back as a warning string.
pub async fn record_duplicate_relationship(
    entry: &Entry,
    client: &HydrusClient,
    settings: Option<&Settings>,
    local_hash: Option<&str>,
    downloaded: &[u8],
    new_hash: &str,
) -> Result<Option<String>, HydrusError> {
    if !settings.is_some_and(|s| s.set_hydrus_duplicate_relationships) {
        return Ok(None);
    }
    let Some(local_hash) = local_hash.filter(|h| !h.is_empty()) else {
        return Ok(None);
    };
    if local_hash.to_lowercase() == new_hash.to_lowercase() {
        // Hydrus turned out to already hold this exact file: one file, so no relationship.
        return Ok(None);
    }

    let Some(verdict) = duplicate_verdict(Path::new(&entry.path), downloaded) else {
        info!(
            "Not relating {} to the local copy in Hydrus: the comparison isn't confident enough \
             to record one",
            entry.filename()
        );
        return Ok(None);
    };

    // Hydrus answers 200 for a pairing against a hash it has never seen, so the local copy has to
    // be confirmed held first.
    let states = client.deletion_states(&[local_hash.to_owned()]).await?;
    let state = states.get(local_hash);
    if state != Some(DeletionState::Present) {
        info!(
            "Not relating {} to the local copy in Hydrus: it isn't holding that file ({})",
            entry.filename(),
            state.map_or("None", DeletionState::as_str)
        );
        return Ok(None);
    }

    if let Some(missing) = client
        .missing_permission(PERMISSION_EDIT_FILE_RELATIONSHIPS)
        .await?
    {
        warn!(
            "Could not relate {} to the local copy in Hydrus: {missing}",
            entry.filename()
        );
        return Ok(Some(format!(
            "Imported, but not marked as a duplicate: {missing}"
        )));
    }

    let related = match verdict {
        Verdict::SameImage => {
            let set = client
                .set_file_relationship(new_hash, local_hash, DUPLICATE_BETTER, true)
                .await;
            // 'Set A as better' alone does not guarantee the new copy ends up king, and being the
            // king is what makes it the one Hydrus shows for the pair.
            match set {
                Ok(()) => client.set_kings(&[new_hash.to_owned()]).await,
                Err(e) => Err(e),
            }
        }
        Verdict::Alternate => {
            client
                .set_file_relationship(new_hash, local_hash, DUPLICATE_ALTERNATE, true)
                .await
        }
    };
    match related {
        Ok(()) => {}
        Err(e) if e.is_hydrus_error_in_python() => {
            warn!(
                "Could not relate {} to the local copy in Hydrus: {e}",
                entry.filename()
            );
            return Ok(Some(format!("Imported, but not marked as a duplicate: {e}")));
        }
        Err(e) => return Err(e),
    }

    info!(
        "Hydrus now has {} as {} of the local copy {}",
        head(new_hash, 12),
        match verdict {
            Verdict::SameImage => "the better duplicate",
            Verdict::Alternate => "an alternate",
        },
        head(local_hash, 12)
    );
    Ok(None)
}

/// `write_provenance_note`: records where this match came from as a note on the file in Hydrus.
///
/// Off unless the user asked for it, checked first: with the feature off this costs no request.
/// Never fails the import; a problem worth telling the user about comes back as a warning string.
pub async fn write_provenance_note(
    entry: &Entry,
    client: &HydrusClient,
    settings: Option<&Settings>,
    file_hash: &str,
    clock: &dyn Clock,
) -> Result<Option<String>, HydrusError> {
    let Some(settings) = settings.filter(|s| s.write_hydrus_provenance_note) else {
        return Ok(None);
    };
    if file_hash.is_empty() {
        return Ok(None);
    }

    let Some(text) = provenance_note(entry, || {
        // `%Y-%m-%d %H:%M`: the stamp without its seconds.
        clock.local_stamp(clock.wall()).chars().take(16).collect()
    }) else {
        // No match, so no provenance, and nothing to tell the user: they did not ask for a note
        // about nothing.
        debug!(
            "No provenance note for {}: it has no matched source",
            entry.filename()
        );
        return Ok(None);
    };

    // Hydrus answers 200 for a note written against a hash it has never seen, and `add_file`
    // reports a hash for a file it REFUSED as previously deleted too.
    let states = client.deletion_states(&[file_hash.to_owned()]).await?;
    let state = states.get(file_hash);
    if state != Some(DeletionState::Present) {
        info!(
            "Not writing a provenance note for {}: Hydrus isn't holding that file ({})",
            entry.filename(),
            state.map_or("None", DeletionState::as_str)
        );
        return Ok(None);
    }

    if let Some(missing) = client.missing_permission(PERMISSION_EDIT_FILE_NOTES).await? {
        warn!(
            "Could not write a provenance note for {}: {missing}",
            entry.filename()
        );
        return Ok(Some(format!(
            "Imported, but no source note was written: {missing}"
        )));
    }

    let name = note_name(&settings.hydrus_provenance_note_name);
    match client.set_note(file_hash, &name, &text).await {
        Ok(()) => {}
        Err(e) if e.is_hydrus_error_in_python() => {
            warn!(
                "Could not write a provenance note for {}: {e}",
                entry.filename()
            );
            return Ok(Some(format!("Imported, but no source note was written: {e}")));
        }
        Err(e) => return Err(e),
    }

    info!("Wrote the {name:?} note onto {} in Hydrus", head(file_hash, 12));
    Ok(None)
}

/// `_joined`: several post-import warnings as the one string `ImportResult` carries. Both the
/// relationship and the note can fail independently, and one swallowing the other's message would
/// report a partial success as a complete one.
pub fn joined(warnings: &[Option<&str>]) -> Option<String> {
    let present: Vec<&str> = warnings.iter().flatten().copied().filter(|w| !w.is_empty()).collect();
    if present.is_empty() {
        None
    } else {
        Some(present.join("; "))
    }
}

/// `_imported_bytes`: the imported file's own bytes, read back out of Hydrus.
///
/// The URL-importer path never sees the bytes (Hydrus's own downloader did the fetching), so the
/// only way to compare the result against the local copy is to ask Hydrus for the file it now has.
/// `Ok(None)` instead of an error, like [`record_duplicate_relationship`]: the import has already
/// succeeded and must not be undone by the bookkeeping that follows it.
pub async fn imported_bytes(
    client: &HydrusClient,
    file_hash: &str,
) -> Result<Option<Vec<u8>>, HydrusError> {
    let Some(file_id) = client.file_id_for_hash(file_hash).await? else {
        warn!(
            "Hydrus confirmed importing {} but has no file id for it, so the file can't be read \
             back to compare",
            head(file_hash, 12)
        );
        return Ok(None);
    };
    // Hydrus streams the file to a path, and these are full-resolution originals: a real temp
    // file, removed as soon as the bytes have been read.
    let Ok(tmp) = TempFile::create("hatate-hydrus-relate-") else {
        warn!(
            "Could not read {} back out of Hydrus to compare it: no temporary file",
            head(file_hash, 12)
        );
        return Ok(None);
    };
    match client.download_file(file_id, tmp.path()).await {
        Ok(()) => match std::fs::read(tmp.path()) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) => {
                warn!(
                    "Could not read {} back out of Hydrus to compare it: {e}",
                    head(file_hash, 12)
                );
                Ok(None)
            }
        },
        Err(e) if e.is_hydrus_error_in_python() => {
            warn!(
                "Could not read {} back out of Hydrus to compare it: {e}",
                head(file_hash, 12)
            );
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// `tempfile.mkstemp(prefix=...)` + `os.unlink`: an empty private file in the temp directory,
/// removed when dropped.
struct TempFile(PathBuf);

impl TempFile {
    fn create(prefix: &str) -> std::io::Result<TempFile> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        for _ in 0..64 {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!("{prefix}{:x}{:x}{n:x}", std::process::id(), nanos));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    file.flush()?;
                    return Ok(TempFile(path));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `value[:n]` for a hash in a log line.
fn head(value: &str, n: usize) -> &str {
    match value.char_indices().nth(n) {
        Some((i, _)) => &value[..i],
        None => value,
    }
}
