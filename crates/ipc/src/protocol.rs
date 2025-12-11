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

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::config::standard;

    #[test]
    fn test_session_to_daemon_request_conversion() {
        let session_req: SessionRequest = Request::Select(vec!["file:///a".into()]);
        let daemon_req: DaemonRequest = session_req.into();

        match daemon_req {
            Request::Select(v) => assert_eq!(v, vec!["file:///a"]),
            _ => panic!("Expected Select variant"),
        }
    }

    #[test]
    fn test_session_to_daemon_response_conversion() {
        let session_resp: SessionResponse = Response::Ok;
        let daemon_resp: DaemonResponse = session_resp.into();

        assert!(matches!(daemon_resp, Response::Ok));
    }

    #[test]
    fn test_wire_compatibility_base_variants() {
        let cfg = standard();

        // Base variants should encode identically
        let session_clear: SessionRequest = Request::Clear;
        let daemon_clear: DaemonRequest = Request::Clear;

        let session_bytes = bincode::encode_to_vec(&session_clear, cfg).unwrap();
        let daemon_bytes = bincode::encode_to_vec(&daemon_clear, cfg).unwrap();

        assert_eq!(session_bytes, daemon_bytes, "Base variants should be wire-compatible");
    }

    #[test]
    fn test_wire_compatibility_select() {
        let cfg = standard();

        let session_req: SessionRequest = Request::Select(vec!["file:///test".into()]);
        let daemon_req: DaemonRequest = Request::Select(vec!["file:///test".into()]);

        let session_bytes = bincode::encode_to_vec(&session_req, cfg).unwrap();
        let daemon_bytes = bincode::encode_to_vec(&daemon_req, cfg).unwrap();

        assert_eq!(session_bytes, daemon_bytes);
    }

    #[test]
    fn test_daemon_extension_encoding() {
        let cfg = standard();

        let list: DaemonRequest = Request::Extended(DaemonExtension::ListSessions);
        let bytes = bincode::encode_to_vec(&list, cfg).unwrap();

        // Extended variant tag (7) + ListSessions tag (0)
        assert_eq!(bytes[0], 7, "Extended variant should be tag 7");
        assert_eq!(bytes[1], 0, "ListSessions should be tag 0");
    }

    #[test]
    fn test_roundtrip_session_request() {
        let cfg = standard();

        let original: SessionRequest = Request::Select(vec!["a".into(), "b".into()]);
        let bytes = bincode::encode_to_vec(&original, cfg).unwrap();
        let (decoded, _): (SessionRequest, _) = bincode::decode_from_slice(&bytes, cfg).unwrap();

        match decoded {
            Request::Select(v) => assert_eq!(v, vec!["a", "b"]),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_roundtrip_daemon_request() {
        let cfg = standard();

        let original: DaemonRequest = Request::Extended(DaemonExtension::GetSession("test-id".into()));
        let bytes = bincode::encode_to_vec(&original, cfg).unwrap();
        let (decoded, _): (DaemonRequest, _) = bincode::decode_from_slice(&bytes, cfg).unwrap();

        match decoded {
            Request::Extended(DaemonExtension::GetSession(id)) => assert_eq!(id, "test-id"),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_roundtrip_daemon_response() {
        let cfg = standard();

        let info = SessionInfo {
            id: "sess-1".into(),
            portal: PortalType::FileChooser,
            title: Some("Pick a file".into()),
            created: 1234567890,
            socket_path: "/tmp/test.sock".into(),
        };

        let original: DaemonResponse = Response::Extended(DaemonResponseExtension::Session(info));
        let bytes = bincode::encode_to_vec(&original, cfg).unwrap();
        let (decoded, _): (DaemonResponse, _) = bincode::decode_from_slice(&bytes, cfg).unwrap();

        match decoded {
            Response::Extended(DaemonResponseExtension::Session(s)) => {
                assert_eq!(s.id, "sess-1");
                assert_eq!(s.portal, PortalType::FileChooser);
                assert_eq!(s.title, Some("Pick a file".into()));
                assert_eq!(s.created, 1234567890);
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_queue_status_roundtrip() {
        let cfg = standard();

        let status = QueueStatusInfo {
            pending_count: 2,
            pending: vec![
                QueuedCommand::Select(vec!["a".into()]),
                QueuedCommand::Clear,
            ],
            submissions_count: 0,
            submissions: vec![],
        };

        let original: DaemonResponse = Response::Extended(DaemonResponseExtension::QueueStatus(status));
        let bytes = bincode::encode_to_vec(&original, cfg).unwrap();
        let (decoded, _): (DaemonResponse, _) = bincode::decode_from_slice(&bytes, cfg).unwrap();

        match decoded {
            Response::Extended(DaemonResponseExtension::QueueStatus(s)) => {
                assert_eq!(s.pending_count, 2);
                assert_eq!(s.pending.len(), 2);
            }
            _ => panic!("Wrong variant"),
        }
    }
}
