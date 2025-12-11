//! Daemon control socket and FIFO for CLI communication
//!
//! Listens on /tmp/portty/<uid>/daemon.sock for CLI requests.
//! Listens on /tmp/portty/<uid>/daemon.ctl for fire-and-forget commands.
//! Owns the session registry. Data operations (edit, clear) are file-based (CLI handles directly).
//! This socket handles control commands: submit, cancel, verify, reset, list.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread;

use libportty::codec::{read_request, write_response};
use libportty::{Request, Response, SessionInfo};
use libportty::{files, paths};
use tracing::{debug, info, warn};

use crate::portal;
use crate::session::{Session, SessionControl, drain_pending_to};

/// Registry of active portal sessions
#[derive(Debug, Default)]
pub struct SessionRegistry {
    sessions: HashMap<String, RegisteredSession>,
}

/// Metadata about a registered session
pub struct RegisteredSession {
    pub id: String,
    pub portal: String,
    pub operation: String,
    pub title: Option<String>,
    pub created: u64,
    pub dir: PathBuf,
    pub control: Arc<SessionControl>,
    pub initial_entries: Vec<String>,
}

impl std::fmt::Debug for RegisteredSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredSession")
            .field("id", &self.id)
            .field("portal", &self.portal)
            .field("operation", &self.operation)
            .field("title", &self.title)
            .field("created", &self.created)
            .field("dir", &self.dir)
            .finish()
    }
}

impl From<&RegisteredSession> for SessionInfo {
    fn from(s: &RegisteredSession) -> Self {
        SessionInfo {
            id: s.id.clone(),
            portal: s.portal.clone(),
            operation: s.operation.clone(),
            title: s.title.clone(),
            created: s.created,
            dir: s.dir.to_string_lossy().into_owned(),
        }
    }
}

impl SessionRegistry {
    /// Create a new session and register it atomically.
    ///
    /// Returns the session (for spawning + running) and the session ID.
    pub fn create_session(
        &mut self,
        portal: &str,
        operation: &str,
        options: &serde_json::Value,
        initial_entries: &[String],
        custom_bins: &HashMap<String, String>,
        title: Option<&str>,
    ) -> std::io::Result<Session> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let control = SessionControl::new(sender.clone());

        let session = Session::new(
            portal,
            operation,
            options,
            initial_entries,
            custom_bins,
            sender,
            receiver,
        )?;

        self.register(RegisteredSession {
            id: session.id().to_string(),
            portal: portal.to_string(),
            operation: operation.to_string(),
            title: title.map(String::from),
            created: session.created(),
            dir: session.dir().to_path_buf(),
            control: Arc::new(control),
            initial_entries: initial_entries.to_vec(),
        });

        Ok(session)
    }

    fn register(&mut self, session: RegisteredSession) {
        info!(id = %session.id, portal = %session.portal, operation = %session.operation, "Registering session");
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
}

/// Shared daemon state
#[derive(Default)]
pub struct DaemonState {
    pub sessions: SessionRegistry,
}

impl DaemonState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Daemon control socket server
pub struct DaemonSocket {
    state: Arc<RwLock<DaemonState>>,
    listener: UnixListener,
}

impl DaemonSocket {
    pub fn new(state: Arc<RwLock<DaemonState>>) -> std::io::Result<Self> {
        paths::ensure_base_dir()?;

        let sock_path = paths::daemon_socket_path();
        let _ = fs::remove_file(&sock_path);

        let listener = UnixListener::bind(&sock_path)?;
        info!(?sock_path, "Daemon socket listening");

        Ok(Self { state, listener })
    }

    pub fn spawn(self) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            for stream in self.listener.incoming() {
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
        })
    }
}

/// Daemon control FIFO for fire-and-forget commands
pub struct DaemonCtl {
    state: Arc<RwLock<DaemonState>>,
}

impl DaemonCtl {
    pub fn new(state: Arc<RwLock<DaemonState>>) -> std::io::Result<Self> {
        paths::ensure_base_dir()?;

        let ctl_path = paths::daemon_ctl_path();
        let _ = fs::remove_file(&ctl_path);

        std::os::unix::fs::mkfifo(&ctl_path, std::fs::Permissions::from_mode(0o600))?;

        info!(?ctl_path, "Daemon FIFO created");

        Ok(Self { state })
    }

    pub fn spawn(self) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let ctl_path = paths::daemon_ctl_path();

            // Open with read+write to prevent EOF when all writers close
            let file = match fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&ctl_path)
            {
                Ok(f) => f,
                Err(e) => {
                    warn!("Failed to open FIFO: {e}");
                    return;
                }
            };
            let reader = BufReader::new(file);

            info!("Daemon FIFO listening");

            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        match Request::decode(&line) {
                            Ok(req) => {
                                debug!(?req, "FIFO request");
                                let resp = handle_request(req, &self.state);
                                debug!(?resp, "FIFO response (discarded)");
                            }
                            Err(e) => {
                                warn!("FIFO parse error: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        warn!("FIFO read error: {e}");
                        break;
                    }
                }
            }
        })
    }
}

fn handle_connection(
    stream: UnixStream,
    state: Arc<RwLock<DaemonState>>,
) -> Result<(), libportty::codec::IpcError> {
    let mut reader = BufReader::new(&stream);
    let mut writer = &stream;

    let req = read_request(&mut reader)?;
    debug!(?req, "Received daemon request");

    let resp = handle_request(req, &state);
    write_response(&mut writer, &resp)?;

    Ok(())
}

fn handle_request(req: Request, state: &Arc<RwLock<DaemonState>>) -> Response {
    match req {
        Request::Submit { session_id } => handle_submit(session_id, state),
        Request::Cancel { session_id } => handle_cancel(session_id, state),
        Request::Verify { session_id } => handle_verify(session_id, state),
        Request::Reset { session_id } => handle_reset(session_id, state),
        Request::List => handle_list(state),
    }
}

/// Submit: resolve session (by id or earliest), drain pending, signal submitted.
/// No session -> queue to submissions dir.
fn handle_submit(session_id: Option<String>, state: &Arc<RwLock<DaemonState>>) -> Response {
    let st = state.read().unwrap_or_else(|e| e.into_inner());

    let session = resolve_session(&st, session_id.as_deref());

    if let Some(session) = session {
        drain_pending_to(&session.dir);
        session.control.submit();
        info!(session_id = %session.id, "Signalled submit");
        Response::Ok
    } else {
        drop(st);
        move_pending_to_submissions()
    }
}

/// Cancel: resolve session, signal cancelled. No session -> clear pending.
fn handle_cancel(session_id: Option<String>, state: &Arc<RwLock<DaemonState>>) -> Response {
    let st = state.read().unwrap_or_else(|e| e.into_inner());

    let session = resolve_session(&st, session_id.as_deref());

    if let Some(session) = session {
        session.control.cancel();
        info!(session_id = %session.id, "Signalled cancel");
        Response::Ok
    } else {
        drop(st);
        let pending_sub = paths::pending_dir().join("submission");
        let _ = fs::write(&pending_sub, "");
        Response::Ok
    }
}

/// Verify: resolve session, read submission + options.json, validate.
fn handle_verify(session_id: Option<String>, state: &Arc<RwLock<DaemonState>>) -> Response {
    let st = state.read().unwrap_or_else(|e| e.into_inner());

    let session = match resolve_session(&st, session_id.as_deref()) {
        Some(s) => s,
        None => return Response::Error("No active session to verify".to_string()),
    };

    let session_dir = session.dir.clone();
    let portal = session.portal.clone();
    let operation = session.operation.clone();
    drop(st);

    // Read submission
    let entries = files::read_lines(&session_dir.join("submission"));

    // Read options.json
    let options: serde_json::Value = match fs::read_to_string(session_dir.join("options.json")) {
        Ok(json) => match serde_json::from_str(&json) {
            Ok(v) => v,
            Err(e) => return Response::Error(format!("Failed to parse options: {e}")),
        },
        Err(e) => return Response::Error(format!("Failed to read options: {e}")),
    };

    match portal::validate(&portal, &operation, &entries, &options) {
        Ok(_) => Response::Ok,
        Err(msg) => Response::Error(msg),
    }
}

/// Reset: resolve session, rewrite submission with initial entries.
fn handle_reset(session_id: Option<String>, state: &Arc<RwLock<DaemonState>>) -> Response {
    let st = state.read().unwrap_or_else(|e| e.into_inner());

    let session = match resolve_session(&st, session_id.as_deref()) {
        Some(s) => s,
        None => return Response::Error("No active session to reset".to_string()),
    };

    let sub_path = session.dir.join("submission");
    let entries = session.initial_entries.clone();
    let sid = session.id.clone();
    drop(st);

    match files::write_lines(&sub_path, &entries) {
        Ok(()) => {
            info!(session_id = %sid, "Reset submission to initial state");
            Response::Ok
        }
        Err(e) => Response::Error(format!("Failed to reset: {e}")),
    }
}

/// List all active sessions.
fn handle_list(state: &Arc<RwLock<DaemonState>>) -> Response {
    let st = state.read().unwrap_or_else(|e| e.into_inner());
    let sessions = st.sessions.iter().map(SessionInfo::from).collect();
    Response::Sessions(sessions)
}

/// Resolve a session: by ID, or earliest if None.
fn resolve_session<'a>(
    state: &'a DaemonState,
    session_id: Option<&str>,
) -> Option<&'a RegisteredSession> {
    match session_id {
        Some(id) => state.sessions.get(id),
        None => state.sessions.iter().min_by_key(|s| s.created),
    }
}

/// Move pending/submission -> submissions/<ts>-any/submission
fn move_pending_to_submissions() -> Response {
    let pending_sub = paths::pending_dir().join("submission");
    let entries = files::read_lines(&pending_sub);

    if entries.is_empty() {
        return Response::Error("No pending entries to submit".to_string());
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let sub_dir = paths::submissions_dir().join(format!("{}-any", ts));
    if let Err(e) = fs::create_dir_all(&sub_dir) {
        return Response::Error(format!("Failed to create submission dir: {e}"));
    }

    if let Err(e) = files::write_lines(&sub_dir.join("submission"), &entries) {
        return Response::Error(format!("Failed to write submission: {e}"));
    }

    let _ = fs::write(&pending_sub, "");
    info!(entries = entries.len(), "Created submission");
    Response::Ok
}
