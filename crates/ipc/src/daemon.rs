//! CLI-daemon protocol
//!
//! Protocol for communication between the `portty` CLI and `porttyd` daemon
//! via the daemon control socket (daemon.sock).

use serde::{Deserialize, Serialize};

/// Request from CLI to daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonRequest {
    /// List all active sessions
    ListSessions,

    /// Get info about a specific session
    GetSession(String),

    /// Send a command to a session
    SessionCommand {
        /// Target session ID (None = auto-select)
        session: Option<String>,
        /// The command to execute
        command: CliCommand,
    },
}

/// Abstract CLI command (daemon translates to portal-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CliCommand {
    /// Submit/confirm and close session
    Submit,
    /// Cancel and close session
    Cancel,
    /// Select items (files, regions, etc. depending on portal)
    Select(Vec<String>),
    /// Deselect items
    Deselect(Vec<String>),
    /// Clear all selection
    Clear,
    /// Get current selection
    GetSelection,
    /// Get session options/info
    GetOptions,
}

/// Response from daemon to CLI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonResponse {
    /// List of active sessions
    Sessions(Vec<SessionInfo>),
    /// Single session info
    Session(SessionInfo),
    /// Command execution result
    CommandResult(CommandResult),
    /// Error occurred
    Error(String),
}

/// Information about a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Unique session identifier
    pub id: String,
    /// Portal type (e.g., "file-chooser", "screenshot")
    pub portal: String,
    /// Session title (from portal options)
    pub title: Option<String>,
    /// Unix timestamp when session was created
    pub created: u64,
}

/// Result of executing a command on a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandResult {
    /// Command succeeded
    Ok,
    /// Current selection (response to GetSelection)
    Selection(Vec<String>),
    /// Session options (response to GetOptions) - portal-specific JSON
    Options(String),
    /// Error message
    Error(String),
}
