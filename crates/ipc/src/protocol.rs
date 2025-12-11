//! Unified IPC protocol with extensible request/response types
//!
//! Uses generic extension types for type-safe protocol extension:
//! - Session socket speaks `SessionRequest` / `SessionResponse`
//! - Daemon socket speaks `DaemonRequest` / `DaemonResponse`
//!
//! Base variants have identical wire format, enabling code reuse while
//! maintaining type safety.

use bincode::{Decode, Encode};

use crate::ipc::file_chooser::SessionOptions;
use crate::queue::{QueuedCommand, Submission};
use crate::PortalType;

/// Uninhabited type for no extension - `Extended(NoExtension)` is unconstructible
#[derive(Debug, Clone, Encode, Decode)]
pub enum NoExtension {}

/// Request with extensible variants
#[derive(Debug, Clone, Encode, Decode)]
pub enum Request<Ext> {
    // === Base commands (work on both session and daemon sockets) ===
    /// Add items to selection (URIs/paths)
    Select(Vec<String>),

    /// Remove items from selection
    Deselect(Vec<String>),

    /// Clear all selection
    Clear,

    /// Submit/confirm and close session
    Submit,

    /// Cancel and close session
    Cancel,

    /// Get current session options
    GetOptions,

    /// Get current selection
    GetSelection,

    // === Extension point ===
    /// Extended commands (daemon-specific when Ext = DaemonExtension)
    Extended(Ext),
}

/// Response with extensible variants
#[derive(Debug, Clone, Encode, Decode)]
pub enum Response<Ext> {
    // === Base responses ===
    /// Operation completed successfully
    Ok,

    /// Current selection
    Selection(Vec<String>),

    /// Session options
    Options(SessionOptions),

    /// Error occurred
    Error(String),

    // === Extension point ===
    /// Extended responses (daemon-specific when Ext = DaemonExtension)
    Extended(Ext),
}

// === Daemon extension types ===

/// Daemon-specific request extensions
#[derive(Debug, Clone, Encode, Decode)]
pub enum DaemonExtension {
    /// List all active sessions
    ListSessions,

    /// Get info about a specific session
    GetSession(String),

    /// Add command to pending queue
    QueuePush(QueuedCommand),

    /// Bundle pending into submission
    QueueSubmit {
        /// Target portal type (None = any)
        portal: Option<PortalType>,
    },

    /// Clear pending commands
    QueueClearPending,

    /// Clear all (pending + submissions)
    QueueClearAll,

    /// Get queue status
    QueueStatus,
}

/// Daemon-specific response extensions
#[derive(Debug, Clone, Encode, Decode)]
pub enum DaemonResponseExtension {
    /// List of active sessions
    Sessions(Vec<SessionInfo>),

    /// Single session info
    Session(SessionInfo),

    /// Queue status
    QueueStatus(QueueStatusInfo),
}

/// Information about a session
#[derive(Debug, Clone, Encode, Decode)]
pub struct SessionInfo {
    /// Unique session identifier
    pub id: String,
    /// Portal type
    pub portal: PortalType,
    /// Session title (from portal options)
    pub title: Option<String>,
    /// Unix timestamp when session was created
    pub created: u64,
    /// Path to session socket (for direct CLI connection)
    pub socket_path: String,
}

/// Queue status information
#[derive(Debug, Clone, Encode, Decode)]
pub struct QueueStatusInfo {
    /// Number of pending commands
    pub pending_count: usize,
    /// Pending commands
    pub pending: Vec<QueuedCommand>,
    /// Number of submissions waiting
    pub submissions_count: usize,
    /// Submissions waiting
    pub submissions: Vec<Submission>,
}

// === Type aliases for convenience ===

/// Session socket request type (no extensions)
pub type SessionRequest = Request<NoExtension>;

/// Session socket response type (no extensions)
pub type SessionResponse = Response<NoExtension>;

/// Daemon socket request type (with extensions)
pub type DaemonRequest = Request<DaemonExtension>;

/// Daemon socket response type (with extensions)
pub type DaemonResponse = Response<DaemonResponseExtension>;

// === Conversion helpers ===

impl From<SessionRequest> for DaemonRequest {
    fn from(req: SessionRequest) -> Self {
        match req {
            Request::Select(v) => Request::Select(v),
            Request::Deselect(v) => Request::Deselect(v),
            Request::Clear => Request::Clear,
            Request::Submit => Request::Submit,
            Request::Cancel => Request::Cancel,
            Request::GetOptions => Request::GetOptions,
            Request::GetSelection => Request::GetSelection,
            Request::Extended(never) => match never {},
        }
    }
}

impl From<SessionResponse> for DaemonResponse {
    fn from(resp: SessionResponse) -> Self {
        match resp {
            Response::Ok => Response::Ok,
            Response::Selection(v) => Response::Selection(v),
            Response::Options(o) => Response::Options(o),
            Response::Error(e) => Response::Error(e),
            Response::Extended(never) => match never {},
        }
    }
}
