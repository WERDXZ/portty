//! Daemon control socket for CLI communication
//!
//! Listens on /tmp/portty/<uid>/daemon.sock for CLI requests.
//! Owns the submission queue and session registry.

use std::collections::HashMap;
use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::thread;

use portty_ipc::ipc::{read_message, write_message, IpcError};
use portty_ipc::queue::{QueuedCommand, SubmissionQueue};
use portty_ipc::{
    DaemonExtension, DaemonRequest, DaemonResponse, DaemonResponseExtension, PortalType,
    QueueStatusInfo, Request, Response, SessionInfo, SessionRequest, SessionResponse,
};
use tracing::{debug, info, warn};

use crate::session::base_dir;

/// Error type for daemon socket operations
#[derive(Debug)]
pub enum DaemonError {
    /// IPC protocol error
    Ipc(IpcError),
    /// Failed to connect to session socket
    SessionConnect(std::io::Error),
    /// Session returned an error response
    SessionError(String),
    /// Unexpected response from session
    UnexpectedResponse,
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DaemonError::Ipc(e) => write!(f, "IPC error: {e}"),
            DaemonError::SessionConnect(e) => write!(f, "session connection failed: {e}"),
            DaemonError::SessionError(e) => write!(f, "session error: {e}"),
            DaemonError::UnexpectedResponse => write!(f, "unexpected response from session"),
        }
    }
}

impl std::error::Error for DaemonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DaemonError::Ipc(e) => Some(e),
            DaemonError::SessionConnect(e) => Some(e),
            _ => None,
        }
    }
}

impl From<IpcError> for DaemonError {
    fn from(e: IpcError) -> Self {
        DaemonError::Ipc(e)
    }
}

/// Registry of active portal sessions
///
/// Maps session IDs to their metadata. Used by the daemon to:
/// - Route CLI commands to the correct session
/// - List active sessions for the user
/// - Auto-select when only one session is active
#[derive(Debug, Default)]
pub struct SessionRegistry {
    sessions: HashMap<String, RegisteredSession>,
}

/// Metadata about a registered session
///
/// Stored in the [`SessionRegistry`] for session lookup and management.
#[derive(Debug, Clone)]
pub struct RegisteredSession {
    /// Unique session identifier
    pub id: String,
    /// Type of portal (FileChooser, etc.)
    pub portal: PortalType,
    /// Human-readable title from portal request
    pub title: Option<String>,
    /// Unix timestamp when session was created
    pub created: u64,
    /// Path to the session's Unix socket
    pub socket_path: PathBuf,
}

impl From<&RegisteredSession> for SessionInfo {
    fn from(s: &RegisteredSession) -> Self {
        SessionInfo {
            id: s.id.clone(),
            portal: s.portal,
            title: s.title.clone(),
            created: s.created,
            socket_path: s.socket_path.to_string_lossy().into_owned(),
        }
    }
}

impl SessionRegistry {
    pub fn register(&mut self, session: RegisteredSession) {
        info!(id = %session.id, portal = %session.portal, "Registering session");
        self.sessions.insert(session.id.clone(), session);
    }

    pub fn unregister(&mut self, id: &str) {
        info!(id, "Unregistering session");
        self.sessions.remove(id);
    }

    pub fn get(&self, id: &str) -> Option<&RegisteredSession> {
        self.sessions.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &RegisteredSession> {
        self.sessions.values()
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// Shared daemon state
///
/// Thread-safe state shared between the D-Bus portal handlers and the
/// daemon control socket. Protected by `RwLock` for concurrent access.
#[derive(Default)]
pub struct DaemonState {
    /// Registry of active portal sessions
    pub sessions: SessionRegistry,
    /// Queue of pending commands and submissions
    pub queue: SubmissionQueue,
}

impl DaemonState {
    /// Create a new daemon state with empty session registry and queue
    pub fn new() -> Self {
        Self::default()
    }
}

/// Daemon control socket server
///
/// Listens on `/tmp/portty/<uid>/daemon.sock` for CLI commands.
/// Handles session management, queue operations, and forwards
/// session commands to the appropriate session sockets.
pub struct DaemonSocket {
    state: Arc<RwLock<DaemonState>>,
    listener: Option<UnixListener>,
}

impl DaemonSocket {
    /// Create and bind the daemon socket
    pub fn new(state: Arc<RwLock<DaemonState>>) -> std::io::Result<Self> {
        let base = base_dir();
        fs::create_dir_all(&base)?;

        let sock_path = base.join("daemon.sock");

        // Remove stale socket if exists
        let _ = fs::remove_file(&sock_path);

        let listener = UnixListener::bind(&sock_path)?;
        info!(?sock_path, "Daemon socket listening");

        Ok(Self {
            state,
            listener: Some(listener),
        })
    }

    /// Run the daemon socket server in a background thread
    pub fn spawn(mut self) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            if let Some(listener) = self.listener.take() {
                self.run(listener);
            }
        })
    }

    fn run(&self, listener: UnixListener) {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let state = Arc::clone(&self.state);
                    thread::spawn(move || {
                        if let Err(e) = handle_connection(stream, state) {
                            warn!("Connection error: {e}");
                        }
                    });
                }
                Err(e) => {
                    warn!("Accept error: {e}");
                }
            }
        }
    }
}

fn handle_connection(
    mut stream: UnixStream,
    state: Arc<RwLock<DaemonState>>,
) -> Result<(), DaemonError> {
    let req: DaemonRequest = read_message(&mut stream)?;
    debug!(?req, "Received daemon request");

    let resp = handle_request(req, &state);
    write_message(&mut stream, &resp)?;

    Ok(())
}

fn handle_request(req: DaemonRequest, state: &Arc<RwLock<DaemonState>>) -> DaemonResponse {
    match req {
        // === Base commands: route to active session or queue ===
        Request::Select(uris) => {
            route_session_command(state, Request::Select(uris))
        }
        Request::Deselect(uris) => {
            route_session_command(state, Request::Deselect(uris))
        }
        Request::Clear => {
            route_session_command(state, Request::Clear)
        }
        Request::Submit => {
            route_session_command(state, Request::Submit)
        }
        Request::Cancel => {
            route_session_command(state, Request::Cancel)
        }
        Request::GetOptions => {
            route_session_command(state, Request::GetOptions)
        }
        Request::GetSelection => {
            route_session_command(state, Request::GetSelection)
        }

        // === Daemon-specific extensions ===
        Request::Extended(ext) => handle_daemon_extension(ext, state),
    }
}

/// Route a session command to the active session
fn route_session_command(
    state: &Arc<RwLock<DaemonState>>,
    req: SessionRequest,
) -> DaemonResponse {
    let st = state.read().unwrap();

    // Auto-select session
    let session = if st.sessions.len() == 1 {
        st.sessions.iter().next().cloned()
    } else if st.sessions.is_empty() {
        return Response::Error("No active sessions".to_string());
    } else {
        return Response::Error(format!(
            "Multiple sessions active ({}), specify --session",
            st.sessions.len()
        ));
    };

    let session = match session {
        Some(s) => s,
        None => return Response::Error("No active sessions".to_string()),
    };

    drop(st); // Release lock before socket operations

    // Forward to session
    match send_to_session(&session.socket_path, &req) {
        Ok(resp) => resp.into(), // Convert SessionResponse to DaemonResponse
        Err(e) => Response::Error(e.to_string()),
    }
}

/// Handle daemon-specific extension commands
fn handle_daemon_extension(
    ext: DaemonExtension,
    state: &Arc<RwLock<DaemonState>>,
) -> DaemonResponse {
    match ext {
        DaemonExtension::ListSessions => {
            let st = state.read().unwrap();
            let sessions = st.sessions.iter().map(SessionInfo::from).collect();
            Response::Extended(DaemonResponseExtension::Sessions(sessions))
        }

        DaemonExtension::GetSession(id) => {
            let st = state.read().unwrap();
            match st.sessions.get(&id) {
                Some(s) => Response::Extended(DaemonResponseExtension::Session(s.into())),
                None => Response::Error(format!("Session not found: {id}")),
            }
        }

        DaemonExtension::QueuePush(cmd) => {
            let mut st = state.write().unwrap();
            st.queue.push_command(cmd);
            Response::Ok
        }

        DaemonExtension::QueueSubmit { portal } => {
            let mut st = state.write().unwrap();
            if st.queue.pending.is_empty() {
                return Response::Error("No pending commands to submit".to_string());
            }

            // Find active session matching portal type
            let matching_session = st.sessions.iter().find(|s| {
                portal.is_none_or(|p| s.portal == p)
            }).cloned();

            if let Some(session) = matching_session {
                // Try to apply commands to active session immediately
                let pending = std::mem::take(&mut st.queue.pending);
                drop(st); // Release lock before socket operations

                match apply_to_session(&session.socket_path, &pending) {
                    Ok(()) => {
                        info!(
                            session_id = %session.id,
                            commands = pending.len(),
                            "Applied queued commands to active session"
                        );
                        Response::Ok
                    }
                    Err(e) => {
                        // Failed to apply - restore pending and queue for later
                        warn!("Failed to apply to session: {e}");
                        let mut st = state.write().unwrap();
                        st.queue.pending = pending;
                        st.queue.submit(portal);
                        Response::Ok
                    }
                }
            } else {
                // No active session - queue for later
                st.queue.submit(portal);
                Response::Ok
            }
        }

        DaemonExtension::QueueClearPending => {
            let mut st = state.write().unwrap();
            st.queue.clear_pending();
            Response::Ok
        }

        DaemonExtension::QueueClearAll => {
            let mut st = state.write().unwrap();
            st.queue.clear_all();
            Response::Ok
        }

        DaemonExtension::QueueStatus => {
            let st = state.read().unwrap();
            Response::Extended(DaemonResponseExtension::QueueStatus(QueueStatusInfo {
                pending_count: st.queue.pending.len(),
                pending: st.queue.pending.clone(),
                submissions_count: st.queue.submissions.len(),
                submissions: st.queue.submissions.clone(),
            }))
        }
    }
}

/// Apply queued commands to a session and submit
fn apply_to_session(socket_path: &Path, commands: &[QueuedCommand]) -> Result<(), DaemonError> {
    // Send each command
    for cmd in commands {
        let req: SessionRequest = match cmd {
            QueuedCommand::Select(uris) => Request::Select(uris.clone()),
            QueuedCommand::Deselect(uris) => Request::Deselect(uris.clone()),
            QueuedCommand::Clear => Request::Clear,
        };

        let resp = send_to_session(socket_path, &req)?;
        match resp {
            Response::Ok => {}
            Response::Error(e) => return Err(DaemonError::SessionError(e)),
            _ => return Err(DaemonError::UnexpectedResponse),
        }
    }

    // Send submit
    let resp = send_to_session(socket_path, &Request::Submit)?;
    match resp {
        Response::Ok => Ok(()),
        Response::Error(e) => Err(DaemonError::SessionError(e)),
        _ => Err(DaemonError::UnexpectedResponse),
    }
}

/// Send a request to session socket
fn send_to_session(socket_path: &Path, req: &SessionRequest) -> Result<SessionResponse, DaemonError> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(DaemonError::SessionConnect)?;

    write_message(&mut stream, req)?;
    read_message(&mut stream).map_err(DaemonError::from)
}
