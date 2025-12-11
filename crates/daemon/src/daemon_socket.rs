//! Daemon control socket for CLI communication
//!
//! Listens on /tmp/portty/<uid>/daemon.sock for CLI requests.
//! Provides session discovery - CLI connects to session sockets directly.

use std::collections::HashMap;
use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread;

use portty_ipc::daemon::{DaemonRequest, DaemonResponse, SessionInfo};
use portty_ipc::ipc::{read_message, write_message};
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
    pub portal: String,
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

    pub fn list(&self) -> Vec<&RegisteredSession> {
        self.sessions.values().collect()
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

/// Daemon socket server
pub struct DaemonSocket {
    registry: Arc<RwLock<SessionRegistry>>,
    listener: Option<UnixListener>,
}

impl DaemonSocket {
    /// Create and bind the daemon socket
    pub fn new(registry: Arc<RwLock<SessionRegistry>>) -> std::io::Result<Self> {
        let base = base_dir();
        fs::create_dir_all(&base)?;

        let sock_path = base.join("daemon.sock");

        // Remove stale socket if exists
        let _ = fs::remove_file(&sock_path);

        let listener = UnixListener::bind(&sock_path)?;
        info!(?sock_path, "Daemon socket listening");

        Ok(Self {
            registry,
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
                    let registry = Arc::clone(&self.registry);
                    thread::spawn(move || {
                        if let Err(e) = handle_connection(stream, registry) {
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
    registry: Arc<RwLock<SessionRegistry>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let req: DaemonRequest = read_message(&mut stream)?;
    debug!(?req, "Received daemon request");

    let resp = handle_request(req, &registry);
    write_message(&mut stream, &resp)?;

    Ok(())
}

fn handle_request(req: DaemonRequest, registry: &Arc<RwLock<SessionRegistry>>) -> DaemonResponse {
    match req {
        DaemonRequest::ListSessions => {
            let reg = registry.read().unwrap();
            let sessions = reg
                .list()
                .iter()
                .map(|s| SessionInfo {
                    id: s.id.clone(),
                    portal: s.portal.clone(),
                    title: s.title.clone(),
                    created: s.created,
                    socket_path: s.socket_path.to_string_lossy().into_owned(),
                })
                .collect();
            DaemonResponse::Sessions(sessions)
        }

        DaemonRequest::GetSession(id) => {
            let reg = registry.read().unwrap();
            match reg.get(&id) {
                Some(s) => DaemonResponse::Session(SessionInfo {
                    id: s.id.clone(),
                    portal: s.portal.clone(),
                    title: s.title.clone(),
                    created: s.created,
                    socket_path: s.socket_path.to_string_lossy().into_owned(),
                }),
                None => DaemonResponse::Error(format!("Session not found: {id}")),
            }
        }

        DaemonRequest::SessionCommand { session, command: _ } => {
            // Return session info with socket path - CLI connects directly
            let reg = registry.read().unwrap();

            let target = match session {
                Some(id) => reg.get(&id).cloned(),
                None => {
                    // Auto-select: use only session if exactly one exists
                    if reg.len() == 1 {
                        reg.list().into_iter().next().cloned()
                    } else if reg.is_empty() {
                        return DaemonResponse::Error("No active sessions".to_string());
                    } else {
                        return DaemonResponse::Error(format!(
                            "Multiple sessions active ({}), specify --session",
                            reg.len()
                        ));
                    }
                }
            };

            match target {
                Some(s) => {
                    // Return session info so CLI can connect directly
                    DaemonResponse::Session(SessionInfo {
                        id: s.id.clone(),
                        portal: s.portal.clone(),
                        title: s.title.clone(),
                        created: s.created,
                        socket_path: s.socket_path.to_string_lossy().into_owned(),
                    })
                }
                None => DaemonResponse::Error("Session not found".to_string()),
            }
        }
    }
}
