//! `core/hydrus_client.py`, the write half (Phase 4.b, DAN-2129): `show_files_in_client`,
//! `_relationship_failure`, `set_file_relationship`, `set_kings`, `_notes_failure`, `set_note`,
//! `delete_files`, `undelete_files`, `deletion_states`, `import_file`, `associate_url`,
//! `import_url` and `add_tags`, as more methods of the 4.a [`HydrusClient`]. Every one is built on
//! `post_json`/`post_raw`/`get`, so the transport, the timeout, the access-key header and the
//! error mapping are the read half's, unchanged.
//!
//! Where the cut is: the importers that call these (4.c, 4.d) and the GUI callers (Phase 6) are
//! not here. The shape of a failure is the read half's: [`HydrusError`], with
//! [`HydrusError::message`] Python's `str(exc)` and the two new kinds
//! ([`ErrorKind::FileUnreadable`], [`ErrorKind::ImportRejected`]) for the two failures Python
//! raises without an HTTP status of its own.

use std::collections::HashMap;
use std::path::Path;

use bytes::Bytes;
use serde_json::{Map, Value, json};
use tracing::{debug, error, info, warn};

use super::{
    HydrusClient, HydrusError, PERMISSION_EDIT_FILE_NOTES, PERMISSION_EDIT_FILE_RELATIONSHIPS,
    WHERE_PERMISSIONS_LIVE, dict, dumps_strings, looks_like_service_key, metadata_rows,
    permission_name, type_name,
};
use crate::error::ErrorKind;
use crate::image_prep::os_error_text;
use crate::pycompat::truthy;
use crate::pyrepr::{py_repr, py_str};

/// The "relationship" enum of `/manage_file_relationships/set_file_relationships`, from the Client
/// API docs. 4 is "set A as better": A and B are the same picture and A is the copy to keep.
pub const DUPLICATE_BETTER: i64 = 4;
/// 3 is "set as alternates": one picture in two versions, kept side by side.
pub const DUPLICATE_ALTERNATE: i64 = 3;

/// Hydrus's page-type enum: 6 is a file search page, the only kind that can be handed a set of
/// hashes to display.
const PAGE_TYPE_FILE_SEARCH: i64 = 6;

/// What Hydrus currently thinks of one hash (`deletion_states`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletionState {
    /// Deleted from my files, sitting in the trash.
    Trashed,
    /// Gone from Hydrus's local storage entirely.
    Deleted,
    /// Still held normally.
    Present,
    /// Hydrus has no record of this hash.
    Unknown,
}

impl DeletionState {
    /// Python's own word for it (`"trashed"`, `"deleted"`, `"present"`, `"unknown"`).
    pub fn as_str(self) -> &'static str {
        match self {
            DeletionState::Trashed => "trashed",
            DeletionState::Deleted => "deleted",
            DeletionState::Present => "present",
            DeletionState::Unknown => "unknown",
        }
    }
}

/// The `{hash: state}` dict `deletion_states` returns, in insertion order: every hash asked about
/// first, in the order asked, then any hash the answer named that was not asked about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeletionStates {
    items: Vec<(String, DeletionState)>,
    index: HashMap<String, usize>,
}

impl DeletionStates {
    fn set(&mut self, hash: &str, state: DeletionState) {
        match self.index.get(hash) {
            Some(&at) => self.items[at].1 = state,
            None => {
                self.index.insert(hash.to_owned(), self.items.len());
                self.items.push((hash.to_owned(), state));
            }
        }
    }

    pub fn get(&self, hash: &str) -> Option<DeletionState> {
        self.index.get(hash).map(|&at| self.items[at].1)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// In insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, DeletionState)> {
        self.items.iter().map(|(h, s)| (h.as_str(), *s))
    }
}

/// `str.lower()` on both sides: whether two hashes are the same one.
fn same_hash(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// `_relationship_failure`: a failed `/manage_file_relationships` call, said in words the user
/// can act on. Only the two the user can fix are named; anything else passes through unchanged.
fn relationship_failure(exc: HydrusError) -> HydrusError {
    let name = permission_name(PERMISSION_EDIT_FILE_RELATIONSHIPS).unwrap_or_default();
    match exc.status_code() {
        Some(403) => HydrusError::new(
            ErrorKind::PermissionDenied,
            Some(403),
            format!(
                "your Hydrus access key does not have the \"{name}\" permission, which is \
                 needed to record duplicate relationships. Add it in {WHERE_PERMISSIONS_LIVE}"
            ),
        ),
        Some(404) => HydrusError::new(
            ErrorKind::HttpStatus,
            Some(404),
            "this Hydrus is too old to record duplicate relationships over the Client API \
             (it has no /manage_file_relationships endpoint). Update Hydrus, or turn the \
             setting off in Settings > Hydrus",
        ),
        _ => exc,
    }
}

/// `_notes_failure`: the same two named cases for `/add_notes`, which has its own permission.
fn notes_failure(exc: HydrusError) -> HydrusError {
    let name = permission_name(PERMISSION_EDIT_FILE_NOTES).unwrap_or_default();
    match exc.status_code() {
        Some(403) => HydrusError::new(
            ErrorKind::PermissionDenied,
            Some(403),
            format!(
                "your Hydrus access key does not have the \"{name}\" permission, which is \
                 needed to write a note onto the file. Add it in {WHERE_PERMISSIONS_LIVE}"
            ),
        ),
        Some(404) => HydrusError::new(
            ErrorKind::HttpStatus,
            Some(404),
            "this Hydrus is too old to write file notes over the Client API (it has no \
             /add_notes/set_notes endpoint). Update Hydrus, or turn the setting off in \
             Settings > Hydrus",
        ),
        _ => exc,
    }
}

/// `value in (4, 7)`: Python's `==` between a JSON number and an int. A bool is not one.
fn import_status_is_failure(status: Option<&Value>) -> bool {
    match status {
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f == 4.0 || f == 7.0),
        _ => false,
    }
}

impl HydrusClient {
    /// `show_files_in_client`: opens a new page in the Hydrus GUI showing these files and returns
    /// its page key (`""` when the answer names none). Needs the Manage Pages permission.
    pub async fn show_files_in_client(
        &self,
        hashes: &[String],
        page_name: &str,
    ) -> Result<String, HydrusError> {
        if hashes.is_empty() {
            return Err(HydrusError::local(
                ErrorKind::InvalidInput,
                "No files to show.",
            ));
        }
        let body = json!({
            "page_type": PAGE_TYPE_FILE_SEARCH,
            "page_name": page_name,
            "hashes": hashes,
            "focus_page": true,
        });
        let data = self
            .post_json("/manage_pages/new_page", Some(&body), &[])
            .await?;
        // `(data or {}).get("page_key", "")`
        if !truthy(&data) {
            return Ok(String::new());
        }
        match dict(&data, "get")?.get("page_key") {
            None => Ok(String::new()),
            Some(Value::String(key)) => Ok(key.clone()),
            Some(other) => Err(HydrusError::unexpected(format!(
                "'page_key' is a {} where a string is expected",
                type_name(other)
            ))),
        }
    }

    /// `set_file_relationship`: records how two files Hydrus already holds relate to each other.
    /// `relationship` is Hydrus's own enum ([`DUPLICATE_BETTER`], [`DUPLICATE_ALTERNATE`]).
    /// `delete_a`/`delete_b` are never sent.
    pub async fn set_file_relationship(
        &self,
        hash_a: &str,
        hash_b: &str,
        relationship: i64,
        do_default_content_merge: bool,
    ) -> Result<(), HydrusError> {
        if hash_a.is_empty() || hash_b.is_empty() {
            return Err(HydrusError::local(
                ErrorKind::InvalidInput,
                "Both file hashes are needed to set a duplicate relationship",
            ));
        }
        if same_hash(hash_a, hash_b) {
            return Err(HydrusError::local(
                ErrorKind::InvalidInput,
                "A file cannot be set as a duplicate of itself",
            ));
        }
        let body = json!({
            "relationships": [{
                "hash_a": hash_a,
                "hash_b": hash_b,
                "relationship": relationship,
                "do_default_content_merge": do_default_content_merge,
            }],
        });
        self.post_json(
            "/manage_file_relationships/set_file_relationships",
            Some(&body),
            &[],
        )
        .await
        .map(|_| ())
        .map_err(relationship_failure)
    }

    /// `set_kings`: promotes each file to the king of its own duplicate group. Nothing to do for
    /// an empty list.
    pub async fn set_kings(&self, hashes: &[String]) -> Result<(), HydrusError> {
        if hashes.is_empty() {
            return Ok(());
        }
        let body = json!({ "hashes": hashes });
        self.post_json("/manage_file_relationships/set_kings", Some(&body), &[])
            .await
            .map(|_| ())
            .map_err(relationship_failure)
    }

    /// `set_note`: writes one named note onto a file Hydrus holds. `merge_cleverly` is not sent,
    /// so Hydrus replaces an existing note of the same name.
    pub async fn set_note(
        &self,
        file_hash: &str,
        name: &str,
        text: &str,
    ) -> Result<(), HydrusError> {
        if file_hash.is_empty() {
            return Err(HydrusError::local(
                ErrorKind::InvalidInput,
                "A file hash is needed to write a note",
            ));
        }
        if name.is_empty() {
            return Err(HydrusError::local(
                ErrorKind::InvalidInput,
                "A note name is needed to write a note",
            ));
        }
        let mut notes = Map::new();
        notes.insert(name.to_owned(), Value::String(text.to_owned()));
        let body = json!({ "hash": file_hash, "notes": notes });
        self.post_json("/add_notes/set_notes", Some(&body), &[])
            .await
            .map(|_| ())
            .map_err(notes_failure)
    }

    /// `delete_files`: asks Hydrus to delete these files from its own database (into its trash),
    /// and returns the hashes it asked about. Hashes Hydrus is not holding right now are left
    /// out first: deleting one would write a deletion record that makes Hydrus refuse the file
    /// if it is ever imported.
    pub async fn delete_files(
        &self,
        hashes: &[String],
        reason: Option<&str>,
    ) -> Result<Vec<String>, HydrusError> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let states = self.deletion_states(hashes).await?;
        let deletable: Vec<String> = hashes
            .iter()
            .filter(|h| states.get(h) == Some(DeletionState::Present))
            .cloned()
            .collect();
        let skipped = hashes.len() - deletable.len();
        if skipped > 0 {
            info!(
                "Not deleting {skipped} hash(es) Hydrus isn't holding - deleting one of those \
                 would only record it as deleted"
            );
        }
        if deletable.is_empty() {
            return Ok(Vec::new());
        }
        let mut body = Map::new();
        body.insert(
            "hashes".to_owned(),
            Value::Array(deletable.iter().cloned().map(Value::String).collect()),
        );
        if let Some(reason) = reason.filter(|r| !r.is_empty()) {
            body.insert("reason".to_owned(), Value::String(reason.to_owned()));
        }
        self.post_json("/add_files/delete_files", Some(&Value::Object(body)), &[])
            .await?;
        info!("Asked Hydrus to delete {} file(s)", deletable.len());
        Ok(deletable)
    }

    /// `undelete_files`: pulls files back out of Hydrus's trash.
    pub async fn undelete_files(&self, hashes: &[String]) -> Result<(), HydrusError> {
        if hashes.is_empty() {
            return Ok(());
        }
        let body = json!({ "hashes": hashes });
        self.post_json("/add_files/undelete_files", Some(&body), &[])
            .await
            .map(|_| ())
    }

    /// `deletion_states`: what Hydrus currently thinks of each hash. Used to confirm a delete
    /// rather than assume it. A call that fails as a [`HydrusError`] is "every hash unknown", with
    /// a warning, as in Python.
    pub async fn deletion_states(&self, hashes: &[String]) -> Result<DeletionStates, HydrusError> {
        let mut states = DeletionStates::default();
        if hashes.is_empty() {
            return Ok(states);
        }
        for hash in hashes {
            states.set(hash, DeletionState::Unknown);
        }
        let data = match self
            .get(
                "/get_files/file_metadata",
                &[("hashes", dumps_strings(hashes))],
            )
            .await
        {
            Ok(data) => data,
            Err(e) if e.is_hydrus_error_in_python() => {
                warn!("Could not read deletion states: {e}");
                return Ok(states);
            }
            Err(e) => return Err(e),
        };
        for m in metadata_rows(&data)? {
            let row = dict(&m, "get")?;
            let hash = match row.get("hash") {
                Some(h) if truthy(h) => h,
                _ => continue,
            };
            let hash = match hash {
                Value::String(s) => s.as_str(),
                // `states[h] = ...` with an unhashable key.
                Value::Array(_) | Value::Object(_) => {
                    // CPython 3.14's text.
                    let ty = type_name(hash);
                    return Err(HydrusError::unexpected(format!(
                        "cannot use '{ty}' as a dict key (unhashable type: '{ty}')"
                    )));
                }
                other => {
                    return Err(HydrusError::unexpected(format!(
                        "'hash' is a {} where a string is expected",
                        type_name(other)
                    )));
                }
            };
            let state = if row.get("file_id").is_none_or(Value::is_null) {
                DeletionState::Unknown
            } else if row.get("is_trashed").is_some_and(truthy) {
                DeletionState::Trashed
            } else if row.get("is_deleted").is_some_and(truthy)
                || !row.get("is_local").is_some_and(truthy)
            {
                DeletionState::Deleted
            } else {
                DeletionState::Present
            };
            states.set(hash, state);
        }
        Ok(states)
    }

    /// `import_file`: uploads a file's raw bytes as the POST body of `/add_files/add_file`.
    /// Hydrus answers 200 even for an import it did not take; its own `status` 4 (failed) or 7
    /// (vetoed) is a [`ErrorKind::ImportRejected`] error carrying its `note`.
    pub async fn import_file(&self, path: &Path) -> Result<Value, HydrusError> {
        let shown = path.to_string_lossy().into_owned();
        if shown.contains('\0') {
            // CPython's `open` raises a bare `ValueError` before it touches the disk.
            return Err(HydrusError::unexpected("embedded null byte"));
        }
        let owned = path.to_owned();
        let read = tokio::task::spawn_blocking(move || std::fs::read(&owned)).await;
        let data = match read {
            Ok(Ok(data)) => data,
            Ok(Err(e)) => {
                let text = os_error_text(&e, path);
                error!("import_file could not read {shown}: {text}");
                return Err(HydrusError::new(
                    ErrorKind::FileUnreadable,
                    None,
                    format!("Could not read local file {shown}: {text}"),
                ));
            }
            Err(join) => {
                return Err(HydrusError::local(
                    ErrorKind::FileUnreadable,
                    format!("Could not read local file {shown}: {join}"),
                ));
            }
        };
        let result = self
            .post_raw(
                "/add_files/add_file",
                Bytes::from(data),
                &[("Content-Type", "application/octet-stream")],
            )
            .await?;
        // Hydrus returns HTTP 200 even for an unsuccessful import: "status" 1/2 is ok
        // (imported / already had it), 3 previously deleted, 4 failed, 7 vetoed by an option.
        let row = dict(&result, "get")?;
        let status = row.get("status");
        if import_status_is_failure(status) {
            let note = row
                .get("note")
                .map(py_str)
                .unwrap_or_else(|| "no details given".to_owned());
            error!(
                "add_file rejected {shown}: status={} note={note}",
                status.map(py_str).unwrap_or_default()
            );
            return Err(HydrusError::new(
                ErrorKind::ImportRejected,
                None,
                format!("Hydrus rejected the file: {note}"),
            ));
        }
        Ok(result)
    }

    /// `associate_url`: tags a file already uploaded with its source URL. Downloads nothing. The
    /// key is `url_to_add`; `url` is silently ignored by Hydrus.
    pub async fn associate_url(
        &self,
        url: &str,
        file_hash: Option<&str>,
    ) -> Result<Value, HydrusError> {
        let mut body = Map::new();
        body.insert("url_to_add".to_owned(), Value::String(url.to_owned()));
        if let Some(hash) = file_hash.filter(|h| !h.is_empty()) {
            body.insert("hashes".to_owned(), json!([hash]));
        }
        self.post_json("/add_urls/associate_url", Some(&Value::Object(body)), &[])
            .await
    }

    /// `import_url`: hands the URL to Hydrus's own downloader. `tags` go to `service_key`, or the
    /// settings' tag service, or `"my tags"`; a name that does not resolve to a real key is
    /// imported without the extra tags, with a warning.
    pub async fn import_url(
        &self,
        url: &str,
        tags: &[String],
        service_key: Option<&str>,
        show_destination_page: bool,
        destination_page_name: Option<&str>,
    ) -> Result<Value, HydrusError> {
        let mut body = Map::new();
        body.insert("url".to_owned(), Value::String(url.to_owned()));
        body.insert(
            "show_destination_page".to_owned(),
            Value::Bool(show_destination_page),
        );
        if let Some(name) = destination_page_name.filter(|n| !n.is_empty()) {
            body.insert(
                "destination_page_name".to_owned(),
                Value::String(name.to_owned()),
            );
        }
        if !tags.is_empty() {
            let service_key = service_key
                .filter(|k| !k.is_empty())
                .or(Some(self.settings().tag_service_key.as_str()).filter(|k| !k.is_empty()))
                .unwrap_or("my tags");
            match self
                .resolve_to_service_key(service_key)
                .await?
                .filter(|k| !k.is_empty())
            {
                Some(resolved) => {
                    let mut by_service = Map::new();
                    by_service.insert(
                        resolved,
                        Value::Array(tags.iter().cloned().map(Value::String).collect()),
                    );
                    body.insert(
                        "service_keys_to_additional_tags".to_owned(),
                        Value::Object(by_service),
                    );
                }
                None => {
                    let shown = py_repr(&Value::String(service_key.to_owned()));
                    warn!(
                        "import_url: could not resolve tag service {shown} to a real key, \
                         importing without extra tags"
                    );
                }
            }
        }
        self.post_json("/add_urls/add_url", Some(&Value::Object(body)), &[])
            .await
    }

    /// `add_tags`: adds tags to a file for a tag service, named or keyed. Which JSON field to try
    /// first is a guess ([`looks_like_service_key`]); a failed first attempt is retried once with
    /// the other field, and the second failure is the one returned.
    pub async fn add_tags(
        &self,
        file_hash: &str,
        tags: &[String],
        service_key: Option<&str>,
    ) -> Result<Value, HydrusError> {
        let service_key = service_key
            .filter(|k| !k.is_empty())
            .or(Some(self.settings().tag_service_key.as_str()).filter(|k| !k.is_empty()))
            // Hydrus's default local tag service name.
            .unwrap_or("my tags");
        let tags_json = Value::Array(tags.iter().cloned().map(Value::String).collect());
        let body_for = |field: &str| {
            let mut by_service = Map::new();
            by_service.insert(service_key.to_owned(), tags_json.clone());
            let mut body = Map::new();
            body.insert("hash".to_owned(), Value::String(file_hash.to_owned()));
            body.insert(field.to_owned(), Value::Object(by_service));
            Value::Object(body)
        };
        let key_body = body_for("service_keys_to_tags");
        let name_body = body_for("service_names_to_tags");
        let (primary, fallback) = if looks_like_service_key(service_key) {
            (key_body, name_body)
        } else {
            (name_body, key_body)
        };
        match self
            .post_json("/add_tags/add_tags", Some(&primary), &[])
            .await
        {
            Ok(done) => Ok(done),
            Err(first) if first.is_hydrus_error_in_python() => {
                let shown = py_repr(&Value::String(service_key.to_owned()));
                warn!(
                    "add_tags: first attempt for service {shown} failed ({first}), retrying \
                     with the other field"
                );
                match self
                    .post_json("/add_tags/add_tags", Some(&fallback), &[])
                    .await
                {
                    Ok(done) => Ok(done),
                    Err(second) => {
                        error!(
                            "add_tags: both attempts failed for service {service_key:?}: {second}"
                        );
                        debug!("add_tags: the first failure was: {}", first.detail());
                        Err(second)
                    }
                }
            }
            Err(first) => Err(first),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_4_and_7_are_a_rejected_import_and_a_bool_is_neither() {
        for rejected in [json!(4), json!(7), json!(4.0), json!(7.0)] {
            assert!(import_status_is_failure(Some(&rejected)), "{rejected}");
        }
        for taken in [
            json!(1),
            json!(2),
            json!(3),
            json!(5),
            json!(true),
            json!("4"),
        ] {
            assert!(!import_status_is_failure(Some(&taken)), "{taken}");
        }
        assert!(!import_status_is_failure(None));
    }

    #[test]
    fn a_failure_with_neither_named_status_passes_through_unchanged() {
        let exc = HydrusError::new(
            ErrorKind::HttpStatus,
            Some(409),
            "Hydrus returned HTTP 409: x",
        );
        assert_eq!(relationship_failure(exc.clone()).message(), exc.message());
        assert_eq!(notes_failure(exc.clone()).message(), exc.message());
        let local = HydrusError::local(ErrorKind::InvalidInput, "x");
        assert_eq!(relationship_failure(local).kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn the_two_named_failures_say_which_checkbox_and_which_endpoint() {
        let e403 = HydrusError::new(ErrorKind::PermissionDenied, Some(403), "raw");
        assert!(
            relationship_failure(e403.clone())
                .message()
                .contains("\"Edit File Relationships\" permission")
        );
        assert!(
            notes_failure(e403)
                .message()
                .contains("\"Edit File Notes\" permission")
        );
        let e404 = HydrusError::new(ErrorKind::HttpStatus, Some(404), "raw");
        assert!(
            relationship_failure(e404.clone())
                .message()
                .contains("/manage_file_relationships endpoint")
        );
        assert!(
            notes_failure(e404)
                .message()
                .contains("/add_notes/set_notes endpoint")
        );
    }

    #[test]
    fn deletion_states_keep_the_asked_order_and_the_last_word() {
        let mut s = DeletionStates::default();
        s.set("b", DeletionState::Unknown);
        s.set("a", DeletionState::Unknown);
        s.set("b", DeletionState::Present);
        s.set("c", DeletionState::Trashed);
        let seen: Vec<_> = s.iter().map(|(h, st)| (h, st.as_str())).collect();
        assert_eq!(
            seen,
            vec![("b", "present"), ("a", "unknown"), ("c", "trashed")]
        );
        assert_eq!(s.len(), 3);
        assert_eq!(s.get("zz"), None);
    }
}
