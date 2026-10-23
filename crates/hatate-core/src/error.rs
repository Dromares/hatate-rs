//! The shared error kind (DAN-2116 item 1). Every error type a Phase 4 ticket adds exposes
//! `kind() -> ErrorKind`, `user_message()` (what failed and what to do: no path, URL query,
//! access key or OS text) and `detail()` (the raw text for a log).
//!
//! There is one kind enum for the crate, not one per module. A network failure wraps the net
//! layer's own kind ([`ErrorKind::Net`]) instead of copying its variants. The kinds are a side
//! channel for the caller: nothing here is written to a session or a config, so
//! `Entry.error_message` and `MatchStatus` are unchanged on disk.

use crate::net;

/// What kind of failure an error is, for a caller that decides what to show or whether to retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The request never got an answer: the net layer's own kind says why (could not connect,
    /// TLS refused, timed out, ...).
    Net(net::ErrorKind),
    /// Hydrus answered 401: it rejected the access key.
    AccessKeyRejected,
    /// Hydrus answered 403: the access key lacks a permission.
    PermissionDenied,
    /// Hydrus answered another status of 400 or more.
    HttpStatus,
    /// Hydrus answered, but the body is not shaped the way the client reads it. Python lets such
    /// an answer raise an `AttributeError` or `TypeError` that its own `except HydrusError`
    /// handlers do not catch, and so does this kind.
    UnexpectedResponse,
    /// The call was made with nothing to do (no hashes to show, for example).
    InvalidInput,
    /// A local file could not be written.
    Io,
}
