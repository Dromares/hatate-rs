//! The error shape every Phase 4 error type exposes (DAN-2116 item 1): a stable kind, a message
//! safe to show a user, and the raw detail for a log.
//!
//! * `kind()` returns [`Kind`]: a `#[non_exhaustive]` enum. A network error wraps the net layer's
//!   own kind as [`Kind::Net`] rather than repeating its variants.
//! * `user_message()` says what failed and what to do. It carries no path, no URL, no access key
//!   and no operating-system text.
//! * `detail()` is the raw text: the OS error, the path.
//!
//! None of this changes what is stored on disk: `Entry.error_message` stays one string, written
//! the way Python writes it, and `MatchStatus` gains no variant.

use crate::net;

/// What kind of failure an error is. Add variants as modules need them; match with a wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// A network failure, with the net layer's own classification.
    Net(net::ErrorKind),
    /// A picture could not be read from disk (missing, unreadable, not a file).
    ImageUnreadable,
    /// The bytes are not a picture this build can decode.
    ImageUndecodable,
    /// The picture is larger than the decoder will accept (Pillow's decompression-bomb guard).
    ImageTooLarge,
}

/// The name the Phase 6 contract uses for [`Kind`].
pub use Kind as ErrorKind;
