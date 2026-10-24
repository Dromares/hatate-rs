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
    /// A local SQLite database (the Pawchive index) could not be opened, read or written.
    Database,
    /// The service answered 404: it has no such thing.
    NotFound,
    /// A local file Hatate was about to hand to Hydrus could not be read (missing, unreadable,
    /// not a file).
    FileUnreadable,
    /// Hydrus answered 200 to an import but its own `status` says it did not take the file
    /// (failed, or vetoed by an import option).
    ImportRejected,
    /// A picture could not be read from disk (missing, unreadable, not a file).
    ImageUnreadable,
    /// The bytes are not a picture this build can decode.
    ImageUndecodable,
    /// The picture is larger than the decoder will accept (Pillow's decompression-bomb guard).
    ImageTooLarge,
    /// The matched image could not be downloaded (nothing came back).
    DownloadFailed,
    /// The matched image came back but failed an integrity check (an HTML page, the wrong
    /// format, the wrong size, bytes that do not decode, a file that cannot be hashed).
    DownloadInvalid,
    /// Hydrus stored a file whose hash is not the hash of the bytes Hatate sent.
    HashMismatch,
    /// A name that is not in the MCP tool registry reached a registry lookup.
    McpUnknownTool,
    /// The MCP server could not start (blank token, unusable settings).
    McpServerStart,
    /// The port the MCP server was asked to listen on is taken.
    McpPortBusy,
    /// The window's list did not run a tool's request in time (`MainThreadTimeout`).
    McpQueueTimeout,
    /// The window's list ran a tool's request and it failed, or the channel was shut down.
    McpQueueFailed,
    /// A search engine cannot run in this build or on this machine (Google Lens with no browser).
    /// Latched for the run: the next image would fail the same way.
    EngineUnavailable,
    /// A search engine stood down: the service challenged the request, or the engine failed on
    /// several images in a row.
    EngineBlocked,
    /// A search engine failed on this image and may work on the next.
    EngineFailed,
    /// The picture is in a format the engine does not take and could not be converted to one
    /// (a video, JPEG XL).
    FormatRefused,
    /// A failure that is not one the engine reports for itself: Python lets such an exception
    /// escape the engine's `except` clauses.
    Unexpected,
    /// A call did not finish inside its hard wall-clock deadline (`HardTimeoutError`).
    Timeout,
    /// A URL could not be read: `urlparse` refused it (`ValueError`).
    InvalidUrl,
}
