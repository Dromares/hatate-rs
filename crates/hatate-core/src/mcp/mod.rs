//! The embedded MCP server and its pieces (`core/mcp_server.py`, `core/mcp_tools.py`,
//! `core/mcp_audit.py`, `core/mcp_content.py`, `tools/mcp_stdio_bridge.py`).
//!
//! The wire is FastMCP's streamable-HTTP transport, byte for byte where a client can see it:
//! see `docs/mcp-server.md`. The tool bodies are 5.e's; this module defines the handler trait
//! they implement.

pub mod audit;
pub mod content;
pub mod handlers;
pub mod pyvalidate;
pub mod schema;
pub mod tools;

use crate::error::ErrorKind;

/// The server listens on loopback only, always (`HOST`). Deliberately not a setting: a setting
/// that could be `0.0.0.0` is a setting that, for somebody, eventually will be.
pub const HOST: &str = "127.0.0.1";

/// An MCP-layer failure with the Phase 6 shape: a stable kind, a message for the user that
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
