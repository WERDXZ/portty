pub mod file_chooser;
pub mod screenshot;

use std::sync::{Arc, RwLock};
use tracing::{debug, info};

use crate::dbus::file_chooser::FileChooserError;
use crate::dbus::screenshot::ScreenshotError;

use crate::config::Config;
use crate::daemon_socket::DaemonState;
use crate::session::{SessionResult, drain_pending_to, pop_queued_submission};

pub use file_chooser::TtyFileChooser;
pub use screenshot::TtyScreenshot;

/// Error from running a session
#[derive(Debug)]
pub enum SessionError {
    Cancelled,
    Other(String),
}

impl From<SessionError> for FileChooserError {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::Cancelled => Self::Cancelled,
            SessionError::Other(msg) => Self::Other(msg),
        }
    }
}

impl From<SessionError> for ScreenshotError {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::Cancelled => Self::Cancelled,
            SessionError::Other(msg) => Self::Other(msg),
        }
    }
}

/// Validate and transform a submission.
///
/// Dispatches to per-portal validate functions via libportty.
pub fn validate(
    portal: &str,
    operation: &str,
    entries: &[String],
    options: &serde_json::Value,
) -> Result<Vec<String>, String> {
    libportty::portal::validate(portal, operation, entries, options)
}

/// Generic session runner shared by all portal handlers.
///
/// Handles: queued submission check -> config resolution -> session creation ->
/// registration -> drain pending -> spawn -> poll -> unregister -> return entries.
pub async fn run_session(
    portal: &str,
    operation: &str,
    options: &serde_json::Value,
    initial_entries: &[String],
    title: Option<&str>,
    config: &Arc<Config>,
    state: &Arc<RwLock<DaemonState>>,
) -> Result<Vec<String>, SessionError> {
    // Check for queued submission on disk first
    if let Some(entries) = pop_queued_submission(portal) {
        info!(
            portal,
            operation,
            entries = entries.len(),
            "Found queued submission on disk, auto-applying"
        );

        if entries.is_empty() {
            info!("Queued submission was empty, cancelling");
            return Err(SessionError::Cancelled);
        }

        let entries = validate(portal, operation, &entries, options)
            .map_err(|e| SessionError::Other(format!("queued submission invalid: {e}")))?;
        info!(?entries, "Queued submission applied");
        return Ok(entries);
    }

    // Resolve config
    let exec = config.resolve_exec(portal, operation).map(String::from);
    let bin = config.resolve_bin(portal, operation);

    let headless = exec.is_none();
    if headless {
        info!(
            portal,
            operation, "Starting headless session (use `portty` CLI to interact)"
        );
    } else {
        debug!(?exec, portal, operation, "Creating session");
    }

    let mut session = {
        let mut st = state.write().unwrap_or_else(|e| e.into_inner());
        st.sessions
            .create_session(portal, operation, options, initial_entries, &bin, title)
    }
    .map_err(|e| SessionError::Other(format!("failed to create session: {e}")))?;

    let session_id = session.id().to_string();
    drain_pending_to(session.dir());

    // Spawn process
    if let Some(ref exec) = exec
        && let Err(e) = session.spawn(exec, portal, operation)
    {
        let mut st = state.write().unwrap_or_else(|e| e.into_inner());
        st.sessions.unregister(&session_id);
        return Err(SessionError::Other(format!("failed to spawn: {e}")));
    }

    // Run session on blocking thread pool (properly bridges sync → async)
    let run_result = blocking::unblock(move || session.run()).await;

    // Always unregister session, even if run() errored
    {
        let mut st = state.write().unwrap_or_else(|e| e.into_inner());
        st.sessions.unregister(&session_id);
    }

    let result = run_result.map_err(|e| SessionError::Other(format!("session failed: {e}")))?;

    match result {
        SessionResult::Success { entries } => {
            let entries = validate(portal, operation, &entries, options)
                .map_err(|e| SessionError::Other(format!("submission invalid: {e}")))?;
            info!(
                ?entries,
                portal, operation, "Session completed successfully"
            );
            Ok(entries)
        }
        SessionResult::Cancelled => {
            info!(portal, operation, "Session cancelled");
            Err(SessionError::Cancelled)
        }
    }
}
