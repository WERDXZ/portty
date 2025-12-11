//! Daemon control socket for CLI communication
//!
//! Listens on /tmp/portty/<uid>/daemon.sock for CLI requests.
//! Owns the submission queue and session registry.

use std::collections::HashMap;
use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread;

use portty_ipc::daemon::{DaemonRequest, DaemonResponse, QueueStatusInfo, SessionInfo};
use portty_ipc::ipc::file_chooser::{Request as SessionRequest, Response as SessionResponse};
use portty_ipc::ipc::{read_message, write_message};
use portty_ipc::queue::{QueuedCommand, SubmissionQueue};
use portty_ipc::PortalType;
use tracing::{debug, info, warn};

use crate::session::base_dir;

/// Registry of active sessions
#[derive(Debug, Default)]
pub struct SessionRegistry {
    sessions: HashMap<String, RegisteredSession>,
}

/// Information about a registered session
#[derive(Debug, Clone)]
pub struct RegisteredSession {
    pub id: String,
    pub portal: PortalType,
    pub title: Option<String>,
    pub created: u64,
    pub socket_path: PathBuf,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

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
pub struct DaemonState {
    pub sessions: SessionRegistry,
    pub queue: SubmissionQueue,
}

impl DaemonState {
    pub fn new() -> Self {
        Self {
            sessions: SessionRegistry::new(),
            queue: SubmissionQueue::new(),
        }
    }
}

impl Default for DaemonState {
    fn default() -> Self {
        Self::new()
    }
}

/// Daemon socket server
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
) -> Result<(), Box<dyn std::error::Error>> {
    let req: DaemonRequest = read_message(&mut stream)?;
    debug!(?req, "Received daemon request");

    let resp = handle_request(req, &state);
    write_message(&mut stream, &resp)?;

    Ok(())
}

fn handle_request(req: DaemonRequest, state: &Arc<RwLock<DaemonState>>) -> DaemonResponse {
    match req {
        DaemonRequest::ListSessions => {
            let st = state.read().unwrap();
            let sessions = st
                .sessions
                .iter()
                .map(|s| SessionInfo {
                    id: s.id.clone(),
                    portal: s.portal,
                    title: s.title.clone(),
                    created: s.created,
                    socket_path: s.socket_path.to_string_lossy().into_owned(),
                })
                .collect();
            DaemonResponse::Sessions(sessions)
        }

        DaemonRequest::GetSession(id) => {
            let st = state.read().unwrap();
            match st.sessions.get(&id) {
                Some(s) => DaemonResponse::Session(SessionInfo {
                    id: s.id.clone(),
                    portal: s.portal,
                    title: s.title.clone(),
                    created: s.created,
                    socket_path: s.socket_path.to_string_lossy().into_owned(),
                }),
                None => DaemonResponse::Error(format!("Session not found: {id}")),
            }
        }

        DaemonRequest::SessionCommand { session, command: _ } => {
            // Return session info with socket path - CLI connects directly
            let st = state.read().unwrap();

            let target = match session {
                Some(id) => st.sessions.get(&id).cloned(),
                None => {
                    // Auto-select: use only session if exactly one exists
                    if st.sessions.len() == 1 {
                        st.sessions.iter().next().cloned()
                    } else if st.sessions.is_empty() {
                        return DaemonResponse::Error("No active sessions".to_string());
                    } else {
                        return DaemonResponse::Error(format!(
                            "Multiple sessions active ({}), specify --session",
                            st.sessions.len()
                        ));
                    }
                }
            };

            match target {
                Some(s) => DaemonResponse::Session(SessionInfo {
                    id: s.id.clone(),
                    portal: s.portal,
                    title: s.title.clone(),
                    created: s.created,
                    socket_path: s.socket_path.to_string_lossy().into_owned(),
                }),
                None => DaemonResponse::Error("Session not found".to_string()),
            }
        }

        // Queue operations
        DaemonRequest::QueuePush(cmd) => {
            let mut st = state.write().unwrap();
            st.queue.push_command(cmd);
            DaemonResponse::Ok
        }

        DaemonRequest::QueueSubmit { portal } => {
            let mut st = state.write().unwrap();
            if st.queue.pending.is_empty() {
                return DaemonResponse::Error("No pending commands to submit".to_string());
            }

            // Find active session matching portal type
            let matching_session = st.sessions.iter().find(|s| {
                portal.map_or(true, |p| s.portal == p)
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
                        DaemonResponse::Ok
                    }
                    Err(e) => {
                        // Failed to apply - restore pending and queue for later
                        warn!("Failed to apply to session: {e}");
                        let mut st = state.write().unwrap();
                        st.queue.pending = pending;
                        st.queue.submit(portal);
                        DaemonResponse::Ok
                    }
                }
            } else {
                // No active session - queue for later
                st.queue.submit(portal);
                DaemonResponse::Ok
            }
        }

        DaemonRequest::QueueClearPending => {
            let mut st = state.write().unwrap();
            st.queue.clear_pending();
            DaemonResponse::Ok
        }

        DaemonRequest::QueueClearAll => {
            let mut st = state.write().unwrap();
            st.queue.clear_all();
            DaemonResponse::Ok
        }

        DaemonRequest::QueueStatus => {
            let st = state.read().unwrap();
            DaemonResponse::QueueStatus(QueueStatusInfo {
                pending_count: st.queue.pending.len(),
                pending: st.queue.pending.clone(),
                submissions_count: st.queue.submissions.len(),
                submissions: st.queue.submissions.clone(),
            })
        }
    }
}

/// Apply queued commands to a session and submit
fn apply_to_session(socket_path: &PathBuf, commands: &[QueuedCommand]) -> Result<(), String> {
    // Send each command
    for cmd in commands {
        let req = match cmd {
            QueuedCommand::Select(uris) => SessionRequest::Select(uris.clone()),
            QueuedCommand::Deselect(uris) => SessionRequest::Deselect(uris.clone()),
            QueuedCommand::Clear => SessionRequest::Clear,
        };

        let resp = send_to_session(socket_path, &req)?;
        match resp {
            SessionResponse::Ok => {}
            SessionResponse::Error(e) => return Err(e),
            _ => return Err("Unexpected response".to_string()),
        }
    }

    // Send submit
    let resp = send_to_session(socket_path, &SessionRequest::Submit)?;
    match resp {
        SessionResponse::Ok => Ok(()),
        SessionResponse::Error(e) => Err(e),
        _ => Err("Unexpected response".to_string()),
    }
}

/// Send a request to session socket
fn send_to_session(socket_path: &PathBuf, req: &SessionRequest) -> Result<SessionResponse, String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("Failed to connect to session: {e}"))?;

    write_message(&mut stream, req)
        .map_err(|e| format!("Failed to send: {e}"))?;

    read_message(&mut stream)
        .map_err(|e| format!("Failed to read: {e}"))
}
