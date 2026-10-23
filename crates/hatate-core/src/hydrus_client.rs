//! `core/hydrus_client.py`, the read half (Phase 4.a, DAN-2126): the connection, the services,
//! search, metadata, tags, thumbnails, files and URL info. The write half (relationships, kings,
//! notes, deletes, import, URL import, `add_tags`) is Phase 4.b to 4.d; `post_json` and
//! `post_raw` (`_post`) are here because every write call is built on them.
//!
//! **Transport.** Python calls `requests.get`/`post` straight at the user's own `api_url`, not
//! through `core/net.py`: no 429 retry, no host wait, no rate-limit bookkeeping. This client does
//! the same, through a [`NetClient`] of its own so that `HydrusSettings.verify_ssl` (the one
//! certificate switch in the app) is that client's `verify_tls`. The timeout is
//! `HydrusSettings.timeout`, per connect and per read, like `requests`'. The binaries have no
//! setting, flag or environment variable that redirects this client anywhere but the user's
//! `hydrus.api_url`.
//!
//! **Answers.** Python reads every answer as untyped JSON. Where the body is not the shape the
//! code reads, Python raises an `AttributeError` or `TypeError` that its own
//! `except HydrusError` handlers do not catch. Here that is [`ErrorKind::UnexpectedResponse`],
//! carrying Python's own exception text, and the handlers (`filter_known_hashes`,
//! `file_id_for_hash`, `missing_permission`, `_resolve_to_service_key`) pass it on in the same
//! places. Where a Rust type is narrower than what Python would carry along (a tag that is a
//! number, a size that is a string) the same kind is returned, with a message that says so
//! (`docs/hydrus-client.md`, class C2).

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::Duration;

use bytes::Bytes;
use serde_json::{Map, Value};
use tracing::{debug, error, warn};

use crate::config::HydrusSettings;
use crate::engines::client::urlencode;
use crate::error::ErrorKind;
use crate::image_prep::os_error_text;
use crate::net::{
    HYDRUS_KEY_HEADER, Method, NetClient, NetConfig, NetError, NetRequest, NetResponse, RetryMode,
    Secret,
};
use crate::pycompat::{strip, truthy};
use crate::pyrepr::py_repr;
use crate::pyresponse::PyResponse;
use crate::pyurl::requote_uri;
use crate::remote::secs;

/// Hydrus's numeric permission ids, verbatim from `/request_new_permissions` in its Client API
/// docs: the names on the checkboxes the user ticks.
pub const HYDRUS_PERMISSIONS: [(i64, &str); 14] = [
    (0, "Import and Edit URLs"),
    (1, "Import and Delete Files"),
    (2, "Edit File Tags"),
    (3, "Search for and Fetch Files"),
    (4, "Manage Pages"),
    (5, "Manage Cookies and Headers"),
    (6, "Manage Database"),
    (7, "Edit File Notes"),
    (8, "Edit File Relationships"),
    (9, "Edit File Ratings"),
    (10, "Manage Popups"),
    (11, "Edit File Times"),
    (12, "Commit Pending"),
    (13, "See Local Paths"),
];
pub const PERMISSION_EDIT_FILE_RELATIONSHIPS: i64 = 8;
pub const PERMISSION_EDIT_FILE_NOTES: i64 = 7;

/// Where a permission is granted, in Hydrus's own menu wording.
pub const WHERE_PERMISSIONS_LIVE: &str = "Hydrus: services \u{2192} review services \u{2192} local \u{2192} client api, then edit your access key and tick it";

/// `_looks_like_service_key`: whether this is a hex service key rather than a service name. A
/// best-effort guess (Hydrus's own local tag service key is a short hex string, not a fixed
/// length), used only to pick which JSON field to try first.
pub fn looks_like_service_key(value: &str) -> bool {
    let value = strip(value);
    !value.is_empty()
        && value.chars().count() % 2 == 0
        && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// `HydrusError`: anything that stopped a Hydrus call from doing what it was asked.
///
/// [`HydrusError::status_code`] is the HTTP status Hydrus answered with, where there was one;
/// callers that tell a missing permission from a missing endpoint branch on it.
/// [`HydrusError::message`] is Python's `str(exc)`; [`HydrusError::user_message`] is the text for
/// a person (no URL, query, access key or OS text), and [`HydrusError::detail`] the text for a log.
#[derive(Debug, Clone)]
pub struct HydrusError {
    kind: ErrorKind,
    status_code: Option<u16>,
    message: String,
    detail: String,
}

impl HydrusError {
    fn new(kind: ErrorKind, status_code: Option<u16>, message: impl Into<String>) -> Self {
        let message = message.into();
        HydrusError {
            kind,
            status_code,
            detail: message.clone(),
            message,
        }
    }

    /// A call with no `status_code` that Python raises as `HydrusError("...")`.
    pub(crate) fn local(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self::new(kind, None, message)
    }

    /// `HydrusError(f"Could not reach Hydrus at {api_url}: {exc}")`.
    fn unreachable(api_url: &str, err: &NetError) -> Self {
        Self::net(
            err,
            format!("Could not reach Hydrus at {api_url}: {err}"),
            err,
        )
    }

    fn net(err: &NetError, message: String, cause: &NetError) -> Self {
        HydrusError {
            kind: ErrorKind::Net(err.kind()),
            status_code: None,
            detail: format!("{message} [{:?}] {}", cause.kind(), cause.detail()),
            message,
        }
    }

    /// An answer whose body is not shaped the way the code reads it: Python's `AttributeError`
    /// or `TypeError`, with its text.
    fn unexpected(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::UnexpectedResponse, None, message)
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The HTTP status Hydrus answered with, or `None` for a connection failure or a locally
    /// raised error.
    pub fn status_code(&self) -> Option<u16> {
        self.status_code
    }

    /// Python's `str(exc)`: the text `Entry.error_message` and the logs carry.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// What to show a person: what failed and what to do about it. No URL, query, access key,
    /// path or OS text.
    pub fn user_message(&self) -> String {
        match self.kind {
            ErrorKind::Net(crate::net::ErrorKind::Timeout)
            | ErrorKind::Net(crate::net::ErrorKind::DeadlineExceeded) => {
                "Hydrus did not answer in time. Check that it is running and not busy, then try again."
                    .to_owned()
            }
            ErrorKind::Net(crate::net::ErrorKind::Tls) => {
                "Could not make a secure connection to Hydrus. Check the API URL in Settings > Hydrus, or turn off certificate checking there if Hydrus uses a self-signed certificate."
                    .to_owned()
            }
            ErrorKind::Net(_) => {
                "Could not reach Hydrus. Check that it is running and that the API URL in Settings > Hydrus is right."
                    .to_owned()
            }
            ErrorKind::AccessKeyRejected => {
                "Hydrus rejected the access key. Check it in Settings > Hydrus.".to_owned()
            }
            ErrorKind::PermissionDenied => {
                "The Hydrus access key lacks a permission this action needs. Tick it in Hydrus: services > review services > local > client api."
                    .to_owned()
            }
            ErrorKind::HttpStatus => match self.status_code {
                Some(code) => format!(
                    "Hydrus answered with an error (HTTP {code}). Try again, or look at Hydrus's own log."
                ),
                None => "Hydrus answered with an error. Try again, or look at Hydrus's own log."
                    .to_owned(),
            },
            ErrorKind::UnexpectedResponse => {
                "Hydrus sent an answer Hatate cannot read. Check that Hydrus and Hatate are up to date."
                    .to_owned()
            }
            ErrorKind::InvalidInput => "There was nothing to send to Hydrus.".to_owned(),
            ErrorKind::Io => {
                "Could not save the file from Hydrus. Check that the destination folder exists and can be written to."
                    .to_owned()
            }
        }
    }

    /// The raw text for a log: Python's message and, for a network failure, the net layer's own
    /// words. Never an access key.
    pub fn detail(&self) -> String {
        self.detail.clone()
    }

    /// Whether Python would have raised this as a `HydrusError`. The other kind
    /// ([`ErrorKind::UnexpectedResponse`]) is a raw `AttributeError`/`TypeError` there, which
    /// the `except HydrusError` handlers let through.
    fn is_hydrus_error_in_python(&self) -> bool {
        self.kind != ErrorKind::UnexpectedResponse
    }
}

impl std::fmt::Display for HydrusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HydrusError {}

/// `HydrusFile`: one row of `/get_files/file_metadata`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HydrusFile {
    pub file_id: Option<i64>,
    pub hash: String,
    pub mime: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub tags: Vec<String>,
}

/// What `get_tags_and_sizes_for_hashes` knows about one hash: its current tags and the size
/// Hydrus has recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashInfo {
    pub tags: Vec<String>,
    pub size: Option<i64>,
}

/// A dict in insertion order, keyed by hash: a hash seen twice keeps its first place and takes
/// its last value, as a Python `dict` does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HashMap2 {
    items: Vec<(String, HashInfo)>,
    index: HashMap<String, usize>,
}

impl HashMap2 {
    fn insert(&mut self, hash: String, info: HashInfo) {
        match self.index.get(&hash) {
            Some(&at) => self.items[at].1 = info,
            None => {
                self.index.insert(hash.clone(), self.items.len());
                self.items.push((hash, info));
            }
        }
    }

    pub fn get(&self, hash: &str) -> Option<&HashInfo> {
        self.index.get(hash).map(|&at| &self.items[at].1)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// In insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &HashInfo)> {
        self.items.iter().map(|(h, i)| (h.as_str(), i))
    }
}

/// The name of a JSON value's Python type, for the text of the exception Python raises on it.
fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `AttributeError: 'list' object has no attribute 'get'`.
fn no_attribute(v: &Value, attr: &str) -> HydrusError {
    HydrusError::unexpected(format!(
        "'{}' object has no attribute '{attr}'",
        type_name(v)
    ))
}

/// `v.get(...)`, `v.items()` or `v.values()`: `v` has to be a dict.
fn dict<'a>(v: &'a Value, attr: &str) -> Result<&'a Map<String, Value>, HydrusError> {
    v.as_object().ok_or_else(|| no_attribute(v, attr))
}

/// `for item in v`: a list's items, a dict's keys, a string's characters.
fn iterate(v: &Value) -> Result<Vec<Value>, HydrusError> {
    match v {
        Value::Array(items) => Ok(items.clone()),
        Value::Object(map) => Ok(map.keys().map(|k| Value::String(k.clone())).collect()),
        Value::String(s) => Ok(s.chars().map(|c| Value::String(c.to_string())).collect()),
        other => Err(HydrusError::unexpected(format!(
            "'{}' object is not iterable",
            type_name(other)
        ))),
    }
}

/// `data.get("metadata", [])`, iterated.
fn metadata_rows(data: &Value) -> Result<Vec<Value>, HydrusError> {
    match dict(data, "get")?.get("metadata") {
        None => Ok(Vec::new()),
        Some(rows) => iterate(rows),
    }
}

/// `_extract_current_tags`: the file's current (status `"0"`) tags, combined across every tag
/// service Hydrus has for it, sorted. `entry` is a metadata row that is already known to be a
/// dict.
fn extract_current_tags(entry: &Map<String, Value>) -> Result<Vec<String>, HydrusError> {
    let by_service: Vec<&Value> = match entry.get("tags") {
        None => Vec::new(),
        Some(v) if !truthy(v) => Vec::new(),
        Some(v) => dict(v, "values")?.values().collect(),
    };
    let mut all_tags: BTreeSet<String> = BTreeSet::new();
    for info in by_service {
        let Value::Object(info) = info else {
            continue;
        };
        let status_map = ["display_tags", "storage_tags"]
            .iter()
            .filter_map(|key| info.get(*key))
            .find(|v| truthy(v));
        let current = match status_map {
            None => None,
            Some(status_map) => dict(status_map, "get")?.get("0"),
        };
        let Some(current) = current.filter(|v| truthy(v)) else {
            continue;
        };
        // `set.update(current)`: a list's items, a dict's keys, a string's characters.
        for item in iterate(current)? {
            match item {
                Value::String(tag) => {
                    all_tags.insert(tag);
                }
                Value::Array(_) | Value::Object(_) => {
                    return Err(HydrusError::unexpected(format!(
                        "unhashable type: '{}'",
                        type_name(&item)
                    )));
                }
                other => {
                    return Err(HydrusError::unexpected(format!(
                        "a tag is a {} where a string is expected",
                        type_name(&other)
                    )));
                }
            }
        }
    }
    Ok(all_tags.into_iter().collect())
}

/// A value Python would keep as it is but that this client holds as an integer.
fn int_field(
    row: &Map<String, Value>,
    key: &str,
) -> Result<Option<i64>, HydrusError> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => {
            Ok(n.as_i64())
        }
        Some(other) => Err(HydrusError::unexpected(format!(
            "'{key}' is a {} where an integer is expected",
            type_name(other)
        ))),
    }
}

fn string_field(
    row: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, HydrusError> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(HydrusError::unexpected(format!(
            "'{key}' is a {} where a string is expected",
            type_name(other)
        ))),
    }
}

/// `json.dumps(value)` as `requests` writes a `json=` body: `", "` and `": "` separators,
/// `ensure_ascii`.
pub(crate) fn py_dumps(value: &Value) -> String {
    let mut out = String::new();
    dump(value, &mut out);
    out
}

fn dump(value: &Value, out: &mut String) {
    use crate::config::pyjson;
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) if n.is_f64() => pyjson::float(out, n.as_f64().unwrap_or(0.0)),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => pyjson::string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dump(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (key, item)) in map.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pyjson::string(out, key);
                out.push_str(": ");
                dump(item, out);
            }
            out.push('}');
        }
    }
}

/// `json.dumps(list_of_strings)`.
fn dumps_strings(items: &[String]) -> String {
    py_dumps(&Value::Array(
        items.iter().map(|s| Value::String(s.clone())).collect(),
    ))
}

/// `text[:n]`, by characters.
fn head(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// `int(x)` on a JSON value, with the exception Python raises when it fails.
fn py_int(v: &Value) -> Result<i64, HydrusError> {
    match v {
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Number(_) => crate::pycompat::int(v).ok_or_else(|| {
            HydrusError::unexpected("cannot convert float infinity to integer".to_owned())
        }),
        Value::String(s) => crate::boorus::sizes::py_int(s).ok_or_else(|| {
            HydrusError::unexpected(format!(
                "invalid literal for int() with base 10: {}",
                py_repr(&Value::String(s.clone()))
            ))
        }),
        other => Err(HydrusError::unexpected(format!(
            "int() argument must be a string, a bytes-like object or a real number, not '{}'",
            type_name(other)
        ))),
    }
}

/// `HydrusClient`: a thin wrapper around the Hydrus Client API.
#[derive(Debug, Clone)]
pub struct HydrusClient {
    settings: HydrusSettings,
    net: NetClient,
}

impl HydrusClient {
    /// Builds the client the binaries use: rustls with the explicit `CryptoProvider`, and
    /// `settings.verify_ssl` as the certificate switch.
    pub fn new(settings: &HydrusSettings) -> Result<Self, HydrusError> {
        Self::with_net_config(settings, NetConfig::default())
    }

    /// As [`HydrusClient::new`] with a net configuration of the caller's (a test adds the root
    /// certificate of its local server). `verify_tls` is always `settings.verify_ssl`.
    pub fn with_net_config(
        settings: &HydrusSettings,
        mut cfg: NetConfig,
    ) -> Result<Self, HydrusError> {
        cfg.verify_tls = settings.verify_ssl;
        let net = NetClient::new(cfg).map_err(|e| HydrusError::unreachable(&settings.api_url, &e))?;
        Ok(HydrusClient {
            settings: settings.clone(),
            net,
        })
    }

    pub fn settings(&self) -> &HydrusSettings {
        &self.settings
    }

    fn timeout(&self) -> Duration {
        secs(self.settings.timeout)
    }

    /// `_headers()` applied to a request: the access key header when a key is set.
    fn with_key(&self, req: NetRequest) -> Result<NetRequest, NetError> {
        if self.settings.access_key.is_empty() {
            return Ok(req);
        }
        let key = Secret::new(self.settings.access_key.expose());
        req.secret_header(HYDRUS_KEY_HEADER, &key)
    }

    /// A request to the user's own Hydrus: not retried and not held back by the host gate, as
    /// Python's direct `requests` calls are not.
    fn request(
        &self,
        method: Method,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<NetRequest, NetError> {
        let mut url = format!("{}{path}", self.settings.api_url);
        if !params.is_empty() {
            let pairs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&urlencode(&pairs));
        }
        let req = NetRequest::new(method, &requote_uri(&url), self.timeout())?
            .with_retry(RetryMode::Never)
            .without_host_gate();
        self.with_key(req)
    }

    async fn exchange(&self, req: Result<NetRequest, NetError>) -> Result<NetResponse, HydrusError> {
        let req = req.map_err(|e| HydrusError::unreachable(&self.settings.api_url, &e))?;
        self.net
            .send(req)
            .await
            .map_err(|e| HydrusError::unreachable(&self.settings.api_url, &e))
    }

    /// `_handle`: an error status becomes a `HydrusError`; a body that is not JSON is `{}`.
    fn handle(path: &str, resp: NetResponse) -> Result<Value, HydrusError> {
        let resp = PyResponse::from_net(resp);
        let status = resp.status;
        debug!("{path} -> HTTP {status}");
        if status == 401 {
            error!("{path} -> 401 Unauthorized: access key rejected");
            return Err(HydrusError::new(
                ErrorKind::AccessKeyRejected,
                Some(401),
                "Hydrus rejected the access key (401) - check Settings > Hydrus",
            ));
        }
        if status == 403 {
            let text = resp.text();
            error!("{path} -> 403 Forbidden: {}", head(&text, 300));
            return Err(HydrusError::new(
                ErrorKind::PermissionDenied,
                Some(403),
                format!(
                    "Access key lacks permission for this action (403): {}",
                    head(&text, 200)
                ),
            ));
        }
        if status >= 400 {
            let text = resp.text();
            error!("{path} -> HTTP {status}: {}", head(&text, 500));
            return Err(HydrusError::new(
                ErrorKind::HttpStatus,
                Some(status),
                format!("Hydrus returned HTTP {status}: {}", head(&text, 300)),
            ));
        }
        Ok(resp
            .json()
            .unwrap_or_else(|_| Value::Object(Map::new())))
    }

    /// `_get`.
    pub async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value, HydrusError> {
        debug!("GET {path}");
        let resp = self.exchange(self.request(Method::Get, path, params)).await?;
        Self::handle(path, resp)
    }

    /// `_post` with a JSON body (`None` and an empty object are both `{}`).
    pub async fn post_json(
        &self,
        path: &str,
        json_body: Option<&Value>,
        extra_headers: &[(&str, &str)],
    ) -> Result<Value, HydrusError> {
        let empty = Value::Object(Map::new());
        let body = json_body.filter(|b| truthy(b)).unwrap_or(&empty);
        debug!("POST {path} body={}", head(&py_dumps(body), 500));
        let text = py_dumps(body);
        let has_content_type = extra_headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"));
        let req = self.request(Method::Post, path, &[]).and_then(|req| {
            let req = if has_content_type {
                req
            } else {
                req.header("Content-Type", "application/json")?
            };
            Self::extra(req, extra_headers).map(|req| req.with_body(Bytes::from(text)))
        });
        let resp = self.exchange(req).await?;
        Self::handle(path, resp)
    }

    /// `_post` with the body's raw bytes (`raw_data`).
    pub async fn post_raw(
        &self,
        path: &str,
        raw_data: Bytes,
        extra_headers: &[(&str, &str)],
    ) -> Result<Value, HydrusError> {
        debug!("POST {path} ({} raw bytes)", raw_data.len());
        let req = self
            .request(Method::Post, path, &[])
            .and_then(|req| Self::extra(req, extra_headers))
            .map(|req| req.with_body(raw_data));
        let resp = self.exchange(req).await?;
        Self::handle(path, resp)
    }

    /// `{**self._headers(), **extra_headers}`: a later header replaces an earlier one of the
    /// same name.
    fn extra(mut req: NetRequest, extra_headers: &[(&str, &str)]) -> Result<NetRequest, NetError> {
        for (name, value) in extra_headers {
            req = req.header(name, value)?;
        }
        Ok(req)
    }

    /// `test_connection`: whether `/verify_access_key` answered with anything.
    pub async fn test_connection(&self) -> Result<bool, HydrusError> {
        let data = self.get("/verify_access_key", &[]).await?;
        Ok(truthy(&data))
    }

    /// `get_services`: the raw `services` object of `/get_services`, `{service_key: {name,
    /// type_pretty, ...}}` (`{}` when the answer has none).
    pub async fn get_services(&self) -> Result<Value, HydrusError> {
        let data = self.get("/get_services", &[]).await?;
        Ok(dict(&data, "get")?
            .get("services")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new())))
    }

    /// `list_tag_services`: `[(service_key, display_name)]` for every service that can hold
    /// tags (local tag services and tag repositories).
    pub async fn list_tag_services(&self) -> Result<Vec<(String, String)>, HydrusError> {
        let services = self.get_services().await?;
        let mut result = Vec::new();
        for (key, info) in dict(&services, "items")? {
            let info = dict(info, "get")?;
            let type_pretty = match info.get("type_pretty") {
                Some(v) if truthy(v) => match v {
                    Value::String(s) => s.to_lowercase(),
                    other => return Err(no_attribute(other, "lower")),
                },
                _ => String::new(),
            };
            if type_pretty.contains("tag") {
                let name = match info.get("name") {
                    None => key.clone(),
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => {
                        return Err(HydrusError::unexpected(format!(
                            "a service name is a {} where a string is expected",
                            type_name(other)
                        )));
                    }
                };
                result.push((key.clone(), name));
            }
        }
        Ok(result)
    }

    /// `search_files`: the ids of the files with all these tags.
    pub async fn search_files(&self, tags: &[String]) -> Result<Vec<i64>, HydrusError> {
        let data = self
            .get(
                "/get_files/search_files",
                &[
                    ("tags", dumps_strings(tags)),
                    ("file_sort_type", "0".to_owned()),
                ],
            )
            .await?;
        match dict(&data, "get")?.get("file_ids") {
            None => Ok(Vec::new()),
            Some(Value::Array(ids)) => ids
                .iter()
                .map(|id| match id {
                    Value::Number(n) if n.is_i64() || n.is_u64() => n.as_i64().ok_or_else(|| {
                        HydrusError::unexpected("a file id is too large for an integer")
                    }),
                    other => Err(HydrusError::unexpected(format!(
                        "a file id is a {} where an integer is expected",
                        type_name(other)
                    ))),
                })
                .collect(),
            Some(other) => Err(HydrusError::unexpected(format!(
                "'file_ids' is a {} where a list is expected",
                type_name(other)
            ))),
        }
    }

    /// `get_file_metadata`: the metadata of these file ids.
    pub async fn get_file_metadata(&self, file_ids: &[i64]) -> Result<Vec<HydrusFile>, HydrusError> {
        let ids = Value::Array(file_ids.iter().map(|id| Value::from(*id)).collect());
        let data = self
            .get(
                "/get_files/file_metadata",
                &[("file_ids", py_dumps(&ids))],
            )
            .await?;
        let mut result = Vec::new();
        for m in metadata_rows(&data)? {
            let row = dict(&m, "get")?;
            let hash = match row.get("hash") {
                None => String::new(),
                Some(Value::String(s)) => s.clone(),
                Some(other) => {
                    return Err(HydrusError::unexpected(format!(
                        "'hash' is a {} where a string is expected",
                        type_name(other)
                    )));
                }
            };
            result.push(HydrusFile {
                file_id: int_field(row, "file_id")?,
                hash,
                mime: string_field(row, "mime")?,
                width: int_field(row, "width")?,
                height: int_field(row, "height")?,
                tags: extract_current_tags(row)?,
            });
        }
        Ok(result)
    }

    /// `get_tags_for_hashes`: `{hash: [tag, ...]}`, only for hashes Hydrus recognises.
    pub async fn get_tags_for_hashes(
        &self,
        hashes: &[String],
    ) -> Result<Vec<(String, Vec<String>)>, HydrusError> {
        let known = self.get_tags_and_sizes_for_hashes(hashes).await?;
        Ok(known.iter().map(|(h, i)| (h.to_owned(), i.tags.clone())).collect())
    }

    /// `get_tags_and_sizes_for_hashes`: `{hash: (tags, size_or_None)}`. The size comes back in the
    /// same response and lets a caller check that a hash identifies the local file it thinks it
    /// does.
    pub async fn get_tags_and_sizes_for_hashes(
        &self,
        hashes: &[String],
    ) -> Result<HashMap2, HydrusError> {
        if hashes.is_empty() {
            return Ok(HashMap2::default());
        }
        let data = self
            .get(
                "/get_files/file_metadata",
                &[("hashes", dumps_strings(hashes))],
            )
            .await?;
        let mut result = HashMap2::default();
        for m in metadata_rows(&data)? {
            let row = dict(&m, "get")?;
            let hash = match row.get("hash") {
                Some(v) if truthy(v) => match v {
                    Value::String(s) => s.clone(),
                    other => {
                        return Err(HydrusError::unexpected(format!(
                            "'hash' is a {} where a string is expected",
                            type_name(other)
                        )));
                    }
                },
                _ => continue,
            };
            result.insert(
                hash,
                HashInfo {
                    tags: extract_current_tags(row)?,
                    size: int_field(row, "size")?,
                },
            );
        }
        Ok(result)
    }

    /// `filter_known_hashes`: which of these hashes Hydrus has. `/get_files/thumbnail` never
    /// 404s (an unknown file gets a generic icon), so a thumbnail fetch must be preceded by
    /// this. A failed call is an empty set, with a warning.
    pub async fn filter_known_hashes(
        &self,
        hashes: &[String],
    ) -> Result<BTreeSet<String>, HydrusError> {
        if hashes.is_empty() {
            return Ok(BTreeSet::new());
        }
        let data = match self
            .get(
                "/get_files/file_metadata",
                &[
                    ("hashes", dumps_strings(hashes)),
                    ("only_return_identifiers", "true".to_owned()),
                ],
            )
            .await
        {
            Ok(data) => data,
            Err(e) if e.is_hydrus_error_in_python() => {
                warn!("filter_known_hashes failed: {e}");
                return Ok(BTreeSet::new());
            }
            Err(e) => return Err(e),
        };
        // A hash Hydrus has never seen still comes back, with a null file_id.
        let mut known = BTreeSet::new();
        for m in metadata_rows(&data)? {
            let row = dict(&m, "get")?;
            let hash = row.get("hash");
            let file_id = row.get("file_id");
            if hash.is_some_and(truthy) && file_id.is_some_and(|v| !v.is_null()) {
                match hash {
                    Some(Value::String(s)) => {
                        known.insert(s.clone());
                    }
                    Some(other) => {
                        return Err(HydrusError::unexpected(format!(
                            "'hash' is a {} where a string is expected",
                            type_name(other)
                        )));
                    }
                    None => {}
                }
            }
        }
        Ok(known)
    }

    /// `missing_permission`: a named, actionable message when the access key definitely does not
    /// hold `permission_id`; `None` when it does, and `None` again when Hydrus cannot be asked
    /// ("inconclusive is not missing").
    pub async fn missing_permission(
        &self,
        permission_id: i64,
    ) -> Result<Option<String>, HydrusError> {
        let info = match self.get("/verify_access_key", &[]).await {
            Ok(info) => info,
            Err(e) if e.is_hydrus_error_in_python() => {
                debug!("Could not read the access key's permissions: {e}");
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let info = dict(&info, "get")?;
        if info.get("permits_everything").is_some_and(truthy) {
            return Ok(None);
        }
        let Some(Value::Array(permissions)) = info.get("basic_permissions") else {
            return Ok(None);
        };
        let holds = permissions.iter().any(|p| match p {
            Value::Bool(b) => i64::from(*b) == permission_id,
            Value::Number(n) => n.as_f64().is_some_and(|f| f == permission_id as f64),
            _ => false,
        });
        if holds {
            return Ok(None);
        }
        let name = HYDRUS_PERMISSIONS
            .iter()
            .find(|(id, _)| *id == permission_id)
            .map(|(_, name)| (*name).to_owned())
            .unwrap_or_else(|| format!("permission {permission_id}"));
        Ok(Some(format!(
            "your Hydrus access key does not have the \"{name}\" permission. Add it in {WHERE_PERMISSIONS_LIVE}"
        )))
    }

    /// `get_thumbnail`, as `fetch_thumbnail`: Hydrus's own stored thumbnail for a file. Python
    /// answers `None` for every failure; this answers a typed [`HydrusError`] (DAN-2116 item 3).
    /// Only call it for hashes [`HydrusClient::filter_known_hashes`] confirmed, or the answer is
    /// Hydrus's fallback icon rather than an error.
    pub async fn fetch_thumbnail(&self, hash: &str) -> Result<Vec<u8>, HydrusError> {
        let req = self.request(Method::Get, "/get_files/thumbnail", &[("hash", hash.to_owned())]);
        let resp = match self.exchange(req).await {
            Ok(resp) => resp,
            Err(e) => {
                debug!("Hydrus thumbnail request failed for {}: {e}", head(hash, 12));
                return Err(e);
            }
        };
        let status = resp.status.as_u16();
        if status != 200 {
            debug!(
                "Hydrus thumbnail for {} returned HTTP {status}",
                head(hash, 12)
            );
            let kind = match status {
                401 => ErrorKind::AccessKeyRejected,
                403 => ErrorKind::PermissionDenied,
                _ => ErrorKind::HttpStatus,
            };
            return Err(HydrusError::new(
                kind,
                Some(status),
                format!(
                    "Hydrus returned HTTP {status} for the thumbnail of {}",
                    head(hash, 12)
                ),
            ));
        }
        Ok(resp.body.to_vec())
    }

    /// `file_id_for_hash`: Hydrus's own numeric id for a file it holds, or `None` for every "no
    /// id" case, a failed call included.
    pub async fn file_id_for_hash(&self, file_hash: &str) -> Result<Option<i64>, HydrusError> {
        if file_hash.is_empty() {
            return Ok(None);
        }
        let ids = Value::Array(vec![Value::String(file_hash.to_owned())]);
        let data = match self
            .get(
                "/get_files/file_metadata",
                &[
                    ("hashes", py_dumps(&ids)),
                    ("only_return_identifiers", "true".to_owned()),
                ],
            )
            .await
        {
            Ok(data) => data,
            Err(e) if e.is_hydrus_error_in_python() => {
                warn!("file_id_for_hash failed for {}: {e}", head(file_hash, 12));
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        for m in metadata_rows(&data)? {
            let row = dict(&m, "get")?;
            let row_hash = match row.get("hash") {
                Some(v) if truthy(v) => match v {
                    Value::String(s) => s.to_lowercase(),
                    other => return Err(no_attribute(other, "lower")),
                },
                _ => String::new(),
            };
            if row_hash != file_hash.to_lowercase() {
                continue;
            }
            return match row.get("file_id") {
                None | Some(Value::Null) => Ok(None),
                Some(id) => Ok(Some(py_int(id)?)),
            };
        }
        Ok(None)
    }

    /// `get_file_path_url`: the URL that serves a file's raw bytes.
    pub fn get_file_path_url(&self, file_id: i64) -> String {
        format!("{}/get_files/file?file_id={file_id}", self.settings.api_url)
    }

    /// `download_file`: the file's bytes, written to `dest_path`.
    pub async fn download_file(&self, file_id: i64, dest_path: &Path) -> Result<(), HydrusError> {
        let req = NetRequest::get(&requote_uri(&self.get_file_path_url(file_id)), self.timeout())
            .map(|req| req.with_retry(RetryMode::Never).without_host_gate())
            .and_then(|req| self.with_key(req));
        let req = req.map_err(|e| {
            HydrusError::local(
                ErrorKind::Net(e.kind()),
                format!("Could not download file {file_id}: {e}"),
            )
        })?;
        let resp = self.net.send(req).await.map_err(|e| {
            error!("download_file {file_id} failed: {e}");
            HydrusError::net(&e, format!("Could not download file {file_id}: {e}"), &e)
        })?;
        let status = resp.status.as_u16();
        if status != 200 {
            error!("download_file {file_id} -> HTTP {status}");
            return Err(HydrusError::new(
                ErrorKind::HttpStatus,
                Some(status),
                format!("Hydrus returned HTTP {status} for file {file_id}"),
            ));
        }
        let path = dest_path.to_owned();
        let body = resp.body;
        let written = tokio::task::spawn_blocking(move || std::fs::write(&path, &body)).await;
        match written {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(HydrusError::new(
                ErrorKind::Io,
                None,
                os_error_text(&e, dest_path),
            )),
            Err(join) => Err(HydrusError::local(
                ErrorKind::Io,
                format!("the file could not be written: {join}"),
            )),
        }
    }

    /// `get_url_info`: how Hydrus classifies a URL before anything is imported: its `url_type`
    /// (0 post, 2 file, 3 gallery, 4 watchable, 5 unknown), the url class that matched, and
    /// whether Hydrus has a parser for it.
    pub async fn get_url_info(&self, url: &str) -> Result<Value, HydrusError> {
        self.get("/add_urls/get_url_info", &[("url", url.to_owned())])
            .await
    }

    /// `get_url_files`: what Hydrus knows about a URL, specifically whether a file has been
    /// imported for it yet. `url_file_statuses` is empty while nothing has been imported and
    /// gains an entry (with a real file hash) once it has.
    pub async fn get_url_files(&self, url: &str) -> Result<Value, HydrusError> {
        self.get("/add_urls/get_url_files", &[("url", url.to_owned())])
            .await
    }

    /// `_resolve_to_service_key`: `import_url`'s `service_keys_to_additional_tags` strictly needs
    /// a hex service key; a plain name is looked up through the services Hydrus lists.
    pub async fn resolve_to_service_key(&self, value: &str) -> Result<Option<String>, HydrusError> {
        if looks_like_service_key(value) {
            return Ok(Some(value.to_owned()));
        }
        match self.list_tag_services().await {
            Ok(services) => {
                let wanted = strip(value).to_lowercase();
                for (key, name) in services {
                    if strip(&name).to_lowercase() == wanted {
                        return Ok(Some(key));
                    }
                }
            }
            Err(e) if e.is_hydrus_error_in_python() => {}
            Err(e) => return Err(e),
        }
        Ok(None)
    }
}
