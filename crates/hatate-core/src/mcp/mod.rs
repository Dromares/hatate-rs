//! The embedded MCP server and its pieces (`core/mcp_server.py`, `core/mcp_tools.py`,
//! `core/mcp_audit.py`, `core/mcp_content.py`, `tools/mcp_stdio_bridge.py`).
//!
//! The wire is FastMCP's streamable-HTTP transport, byte for byte where a client can see it:
//! see `docs/mcp-server.md`. The tool bodies are 5.e's; this module defines the handler trait
//! they implement.

pub mod audit;
pub mod bridge;
pub mod content;
pub mod envelope;
pub mod handlers;
pub mod protocol;
pub mod pyvalidate;
pub mod schema;
pub mod server;
pub mod tools;

use crate::error::ErrorKind;

/// The server listens on loopback only, always (`HOST`). Deliberately not a setting: a setting
/// that could be `0.0.0.0` is a setting that, for somebody, eventually will be.
pub const HOST: &str = "127.0.0.1";

/// An MCP-layer failure with the Phase 6 shape: a fixed kind, a message for the user that
/// carries no path, token or OS text, and the raw detail for a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpError {
    kind: ErrorKind,
    user_message: String,
    detail: String,
}

impl McpError {
    pub(crate) fn unknown_tool(name: &str) -> Self {
        Self {
            kind: ErrorKind::McpUnknownTool,
            user_message: "That MCP tool does not exist.".to_owned(),
            detail: format!("unknown tool {name:?}"),
        }
    }

    /// A server-start failure: `user_message` is what the Settings page can show, `detail` the
    /// Python-matched text kept for the log.
    pub(crate) fn start_failure(kind: ErrorKind, user_message: &str, detail: String) -> Self {
        Self {
            kind,
            user_message: user_message.to_owned(),
            detail,
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// What failed and what to do. No path, token, URL query or OS text.
    pub fn user_message(&self) -> String {
        self.user_message.clone()
    }

    /// The raw text for a log.
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_message)
    }
}

impl std::error::Error for McpError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_shape_has_a_fixed_kind_and_no_raw_text_in_the_user_message() {
        let e = McpError::unknown_tool("rm -rf /home/user/.config?token=synthetic");
        assert_eq!(e.kind(), ErrorKind::McpUnknownTool);
        let user = e.user_message();
        assert!(
            !user.contains("token") && !user.contains("/home") && !user.contains("os error"),
            "{user}"
        );
        assert!(e.detail().contains("unknown tool"));
    }
}
