//! The window's own error types (DAN-2116 item 1): each exposes `kind()`, `user_message()` (what
//! failed and what to do: no path, no OS text) and `detail()` (the raw text, for the log or a
//! detail line). The window shows `user_message()`; `detail()` goes to the log.
//!
//! Python's session dialogs name the file the user chose in their own wording
//! (`Could not write the session to:\n{path}`); those texts are kept as Python has them because
//! the path is the user's own input, not an OS message. These types cover the failures Python
//! lets surface as a raw exception, and the Rust-only refusal to write while the Python app holds
//! the profile.

use hatate_core::error::ErrorKind;
use hatate_core::session::SaveError;
use std::fmt;
use std::path::Path;

/// A session could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionError {
    kind: ErrorKind,
    user_message: &'static str,
    detail: String,
}

impl SessionError {
    /// The file is missing, unreadable, or not a session file this build can read.
    pub fn unreadable(path: &Path, why: &str) -> SessionError {
        SessionError {
            kind: ErrorKind::FileUnreadable,
            user_message: "That file is not a session this build can read. Choose a session \
                           file saved by Hatate, or see Help > View Logs for details.",
            detail: format!("{}: {why}", path.display()),
        }
    }

    /// `session.db` or a named session could not be written.
    pub fn from_save(e: &SaveError) -> SessionError {
        let (kind, user_message) = match e {
            SaveError::Refused(_) => (
                ErrorKind::PermissionDenied,
                "The session was not saved because the Hatate Python app is running. Quit it \
                 first, then try again.",
            ),
            SaveError::Database(_) => (
                ErrorKind::Database,
                "The session could not be saved. See Help > View Logs for details.",
            ),
            SaveError::Io(_) | SaveError::Encode(_) => (
                ErrorKind::Io,
                "The session could not be saved. Check that the folder exists and has room, \
                 then try again.",
            ),
        };
        SessionError {
            kind,
            user_message,
            detail: e.to_string(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn user_message(&self) -> String {
        self.user_message.to_owned()
    }

    pub fn detail(&self) -> String {
        self.detail.clone()
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.user_message)
    }
}

impl std::error::Error for SessionError {}

/// A results file could not be written (Export Results).
#[derive(Debug)]
pub struct ExportError {
    error: std::io::Error,
    path: std::path::PathBuf,
}

impl ExportError {
    pub fn new(path: &Path, error: std::io::Error) -> ExportError {
        ExportError {
            error,
            path: path.to_owned(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Io
    }

    pub fn user_message(&self) -> String {
        "The results could not be written. Check that the folder exists and has room, then try \
         again."
            .to_owned()
    }

    pub fn detail(&self) -> String {
        format!("{}: {}", self.path.display(), self.error)
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for ExportError {}

/// The search cache could not be cleared.
#[derive(Debug)]
pub struct CacheError {
    error: std::io::Error,
}

impl CacheError {
    pub fn new(error: std::io::Error) -> CacheError {
        CacheError { error }
    }

    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Io
    }

    pub fn user_message(&self) -> String {
        "The search cache could not be cleared. See Help > View Logs for details.".to_owned()
    }

    pub fn detail(&self) -> String {
        self.error.to_string()
    }
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for CacheError {}

/// The bundled fonts could not be put where fontconfig can see them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontError {
    detail: String,
}

impl FontError {
    pub fn new(detail: impl Into<String>) -> FontError {
        FontError {
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Io
    }

    pub fn user_message(&self) -> String {
        "The bundled fonts could not be loaded, so the window uses the fonts installed on this \
         system."
            .to_owned()
    }

    pub fn detail(&self) -> String {
        self.detail.clone()
    }
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for FontError {}

#[cfg(test)]
mod tests {
    use super::*;
    use hatate_core::session::guard::PythonAppRunning;

    fn clean(text: &str, path: &str) {
        assert!(!text.contains(path), "path in {text:?}");
        assert!(!text.contains("os error"), "OS text in {text:?}");
        assert!(!text.contains("No such file"), "OS text in {text:?}");
    }

    /// The contract line: a missing-file open yields a `user_message` with neither the path nor
    /// `os error`; the raw text is in `detail()`.
    #[test]
    fn a_missing_file_open_hides_the_path_and_the_os_text() {
        let path = Path::new("/home/someone/private/session.json");
        let e = SessionError::unreadable(path, "No such file or directory (os error 2)");
        assert_eq!(e.kind(), ErrorKind::FileUnreadable);
        clean(&e.user_message(), "/home/someone");
        clean(&e.user_message(), "session.json");
        assert!(e.detail().contains("/home/someone/private/session.json"));
        assert!(e.detail().contains("os error 2"));
        assert_eq!(e.to_string(), e.user_message());
    }

    #[test]
    fn every_save_failure_has_a_clean_message_and_a_raw_detail() {
        let cases = [
            SaveError::Refused(PythonAppRunning { pid: 4242 }),
            SaveError::Database("UNIQUE constraint failed: entries.path".into()),
            SaveError::Io(std::io::Error::other("disk full at /mnt/secret/x.db")),
            SaveError::Encode("bad float".into()),
        ];
        let kinds = [
            ErrorKind::PermissionDenied,
            ErrorKind::Database,
            ErrorKind::Io,
            ErrorKind::Io,
        ];
        for (c, kind) in cases.iter().zip(kinds) {
            let e = SessionError::from_save(c);
            assert_eq!(e.kind(), kind);
            clean(&e.user_message(), "/mnt/secret");
            clean(&e.user_message(), "4242");
            clean(&e.user_message(), "UNIQUE");
            assert!(!e.detail().is_empty());
        }
    }

    #[test]
    fn an_export_or_cache_failure_hides_the_path_and_the_os_text() {
        let os = std::io::Error::from_raw_os_error(2);
        let e = ExportError::new(Path::new("/home/someone/out.csv"), os);
        assert_eq!(e.kind(), ErrorKind::Io);
        clean(&e.user_message(), "/home/someone");
        assert!(e.detail().contains("/home/someone/out.csv") && e.detail().contains("os error 2"));
        let c = CacheError::new(std::io::Error::from_raw_os_error(13));
        clean(&c.user_message(), "denied");
        assert!(c.detail().contains("os error 13"));
    }

    #[test]
    fn a_font_error_names_no_path() {
        let e = FontError::new(
            "font dir /home/x/.cache/hatate-rs/fonts: permission denied (os error 13)",
        );
        assert_eq!(e.kind(), ErrorKind::Io);
        clean(&e.user_message(), "/home/x");
        assert!(e.detail().contains("/home/x"));
    }
}
