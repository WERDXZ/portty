//! Unified IPC protocol with extensible request/response types
//!
//! This module defines the wire protocol for communication between:
//! - CLI → Daemon: Management commands (queue, session listing)
//! - CLI → Session: Direct session control (select, submit, cancel)
//! - Daemon → Session: Forwarded commands
//!
//! # Extension Pattern
//!
//! Uses generic extension types for type-safe protocol extension:
//! - [`SessionRequest`] / [`SessionResponse`]: Base commands only
//! - [`DaemonRequest`] / [`DaemonResponse`]: Base + daemon extensions
//!
//! Base variants share identical wire format, enabling code reuse while
//! maintaining type safety. The [`NoExtension`] type is uninhabited,
//! making `Extended(NoExtension)` unconstructible at compile time.
//!
//! # Wire Format
//!
//! Messages are encoded with bincode using length-prefixed framing:
//! - 4-byte little-endian length prefix
//! - bincode-encoded payload

use bincode::{Decode, Encode};

use crate::ipc::context::PortalContext;
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

    /// Reset selection to initial defaults
    Reset,

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

    /// Session options (portal-specific context)
    Options(PortalContext),

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
            Request::Reset => Request::Reset,
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

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::config::standard;

    #[test]
    fn session_to_daemon_request_conversion() {
        let session_req: SessionRequest = Request::Select(vec!["file:///a".into()]);
        let daemon_req: DaemonRequest = session_req.into();
        assert!(matches!(daemon_req, Request::Select(v) if v == ["file:///a"]));
    }

    #[test]
    fn session_to_daemon_response_conversion() {
        let cases: Vec<SessionResponse> = vec![
            Response::Ok,
            Response::Error("test".into()),
            Response::Selection(vec!["a".into()]),
        ];
        for resp in cases {
            let _: DaemonResponse = resp.into();
        }
    }

    #[test]
    fn wire_compatibility_base_variants() {
        let cfg = standard();

        let variants: Vec<(SessionRequest, DaemonRequest)> = vec![
            (Request::Clear, Request::Clear),
            (Request::Submit, Request::Submit),
            (Request::Cancel, Request::Cancel),
            (Request::Reset, Request::Reset),
            (
                Request::Select(vec!["test".into()]),
                Request::Select(vec!["test".into()]),
            ),
        ];

        for (session, daemon) in variants {
            let session_bytes = bincode::encode_to_vec(&session, cfg).unwrap();
            let daemon_bytes = bincode::encode_to_vec(&daemon, cfg).unwrap();
            assert_eq!(session_bytes, daemon_bytes);
        }
    }

    #[test]
    fn extended_variant_tag_stable() {
        let cfg = standard();
        let list: DaemonRequest = Request::Extended(DaemonExtension::ListSessions);
        let bytes = bincode::encode_to_vec(&list, cfg).unwrap();
        assert_eq!(bytes[0], 8);
    }

    #[test]
    fn protocol_roundtrip() {
        let cfg = standard();

        let req: SessionRequest = Request::Select(vec!["a".into(), "b".into()]);
        let bytes = bincode::encode_to_vec(&req, cfg).unwrap();
        let (decoded, _): (SessionRequest, _) = bincode::decode_from_slice(&bytes, cfg).unwrap();
        assert!(matches!(decoded, Request::Select(v) if v == ["a", "b"]));

        let req: DaemonRequest = Request::Extended(DaemonExtension::GetSession("id".into()));
        let bytes = bincode::encode_to_vec(&req, cfg).unwrap();
        let (decoded, _): (DaemonRequest, _) = bincode::decode_from_slice(&bytes, cfg).unwrap();
        assert!(matches!(decoded, Request::Extended(DaemonExtension::GetSession(id)) if id == "id"));

        let info = SessionInfo {
            id: "sess-1".into(),
            portal: PortalType::FileChooser,
            title: Some("Pick a file".into()),
            created: 1234567890,
            socket_path: "/tmp/test.sock".into(),
        };
        let resp: DaemonResponse = Response::Extended(DaemonResponseExtension::Session(info));
        let bytes = bincode::encode_to_vec(&resp, cfg).unwrap();
        let (decoded, _): (DaemonResponse, _) = bincode::decode_from_slice(&bytes, cfg).unwrap();
        assert!(matches!(decoded, Response::Extended(DaemonResponseExtension::Session(s)) if s.id == "sess-1"));

        let status = QueueStatusInfo {
            pending_count: 2,
            pending: vec![QueuedCommand::Select(vec!["a".into()]), QueuedCommand::Clear],
            submissions_count: 0,
            submissions: vec![],
        };
        let resp: DaemonResponse = Response::Extended(DaemonResponseExtension::QueueStatus(status));
        let bytes = bincode::encode_to_vec(&resp, cfg).unwrap();
        let (decoded, _): (DaemonResponse, _) = bincode::decode_from_slice(&bytes, cfg).unwrap();
        assert!(matches!(decoded, Response::Extended(DaemonResponseExtension::QueueStatus(s)) if s.pending.len() == 2));
    }
}
