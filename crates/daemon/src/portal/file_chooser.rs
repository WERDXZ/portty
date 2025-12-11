use std::sync::{Arc, RwLock};
use std::thread;

use futures_lite::future::yield_now;
use tracing::{debug, info, instrument};

use crate::config::{Config, FileChooserOp};
use crate::daemon_socket::{DaemonState, RegisteredSession};
use crate::session::{Session, SessionResult};
use portty_ipc::ipc::file_chooser::{Filter, FilterPattern, SessionOptions};
use portty_ipc::portal::file_chooser::{
    FileChooserError, FileChooserHandler, FileChooserResult, FileFilter, OpenFileOptions,
    SaveFileOptions, SaveFilesOptions, FilterPattern as PortalFilterPattern,
};
use portty_ipc::queue::QueuedCommand;
use portty_ipc::PortalType;

/// File chooser handler that spawns terminals
pub struct TtyFileChooser {
    config: Arc<Config>,
    state: Arc<RwLock<DaemonState>>,
}

impl TtyFileChooser {
    pub fn new(config: Arc<Config>, state: Arc<RwLock<DaemonState>>) -> Self {
        debug!("FileChooser initialized");
        Self { config, state }
    }

    async fn run_session(
        &self,
        op: FileChooserOp,
        options: SessionOptions,
    ) -> Result<FileChooserResult, FileChooserError> {
        let portal = PortalType::FileChooser;

        // Check for queued submission first
        {
            let mut st = self.state.write().unwrap();
            if let Some(submission) = st.queue.pop_for_portal(portal) {
                info!(
                    ?op,
                    commands = submission.commands.len(),
                    "Found queued submission, auto-applying"
                );

                // Apply queued commands to get URIs
                let mut selection: Vec<String> = Vec::new();
                for cmd in submission.commands {
                    match cmd {
                        QueuedCommand::Select(uris) => {
                            for uri in uris {
                                if !selection.contains(&uri) {
                                    selection.push(uri);
                                }
                            }
                        }
                        QueuedCommand::Deselect(uris) => {
                            selection.retain(|u| !uris.contains(u));
                        }
                        QueuedCommand::Clear => {
                            selection.clear();
                        }
                    }
                }

                if selection.is_empty() {
                    info!("Queued submission resulted in empty selection, cancelling");
                    return Err(FileChooserError::Cancelled);
                }

                info!(?selection, "Queued submission applied");
                return Ok(FileChooserResult::new().uris(selection));
            }
        }

        // Get operation-specific config
        let exec = self.config.file_chooser_exec(op).map(String::from);
        let bin = self.config.file_chooser_bin(op);

        let headless = exec.is_none();
        if headless {
            info!(?op, "Starting headless session (use `portty` CLI to interact)");
        } else {
            debug!(?exec, ?op, "Creating session");
        }

        let mut session = Session::new(portal.as_str(), options, &bin)
            .map_err(|e| FileChooserError::Other(format!("failed to create session: {e}")))?;

        // Register session and transfer any pending commands
        let session_id = session.id().to_string();
        {
            let mut st = self.state.write().unwrap();
            st.sessions.register(RegisteredSession {
                id: session_id.clone(),
                portal,
                title: session.title().map(String::from),
                created: session.created(),
                socket_path: session.socket_path().to_path_buf(),
            });

            // Transfer pending commands to session (user can still review/modify)
            if !st.queue.pending.is_empty() {
                let pending = std::mem::take(&mut st.queue.pending);
                info!(
                    commands = pending.len(),
                    "Transferring pending commands to session"
                );
                session.apply_pending(pending);
            }
        }

        // Spawn process (terminal or auto-confirm command like "submit")
        if let Some(ref exec) = exec {
            session
                .spawn(exec, portal.as_str())
                .map_err(|e| FileChooserError::Other(format!("failed to spawn: {e}")))?;
        }

        // Run session in background thread (allows concurrent sessions)
        let handle = thread::spawn(move || session.run());

        // Poll until thread completes
        loop {
            if handle.is_finished() {
                break;
            }
            yield_now().await;
        }

        let result = handle
            .join()
            .map_err(|_| FileChooserError::Other("session thread panicked".to_string()))?
            .map_err(|e| FileChooserError::Other(format!("session failed: {e}")))?;

        // Unregister session after completion
        {
            let mut st = self.state.write().unwrap();
            st.sessions.unregister(&session_id);
        }

        match result {
            SessionResult::Success { ref uris } => {
                info!(?uris, "Session completed successfully");
                Ok(FileChooserResult::new().uris(uris.clone()))
            }
            SessionResult::Cancelled => {
                info!("Session cancelled");
                Err(FileChooserError::Cancelled)
            }
        }
    }
}

/// Convert D-Bus filters to IPC filters
fn convert_filters(filters: &[FileFilter]) -> Vec<Filter> {
    filters
        .iter()
        .map(|f| Filter {
            name: f.name().to_string(),
            patterns: f
                .patterns()
                .map(|p| match p {
                    PortalFilterPattern::Glob(s) => {
                        FilterPattern::Glob(s.to_string())
                    }
                    PortalFilterPattern::MimeType(s) => {
                        FilterPattern::MimeType(s.to_string())
                    }
                })
                .collect(),
        })
        .collect()
}

impl FileChooserHandler for TtyFileChooser {
    #[instrument(skip(self, _parent_window, options))]
    async fn open_file(
        &self,
        _handle: String,
        _app_id: String,
        _parent_window: String,
        title: String,
        options: OpenFileOptions,
    ) -> Result<FileChooserResult, FileChooserError> {
        info!(
            multiple = ?options.multiple(),
            directory = ?options.directory(),
            "OpenFile request"
        );

        let session_options = SessionOptions {
            title,
            multiple: options.multiple().unwrap_or(false),
            directory: options.directory().unwrap_or(false),
            save_mode: false,
            current_name: None,
            current_folder: options
                .current_folder()
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            filters: convert_filters(options.filters()),
            current_filter: None,
        };

        self.run_session(FileChooserOp::OpenFile, session_options).await
    }

    #[instrument(skip(self, _parent_window, options))]
    async fn save_file(
        &self,
        _handle: String,
        _app_id: String,
        _parent_window: String,
        title: String,
        options: SaveFileOptions,
    ) -> Result<FileChooserResult, FileChooserError> {
        info!(current_name = ?options.current_name(), "SaveFile request");

        let session_options = SessionOptions {
            title,
            multiple: false,
            directory: false,
            save_mode: true,
            current_name: options.current_name().map(String::from),
            current_folder: options
                .current_folder()
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            filters: convert_filters(options.filters()),
            current_filter: None,
        };

        self.run_session(FileChooserOp::SaveFile, session_options).await
    }

    #[instrument(skip(self, _parent_window, options))]
    async fn save_files(
        &self,
        _handle: String,
        _app_id: String,
        _parent_window: String,
        title: String,
        options: SaveFilesOptions,
    ) -> Result<FileChooserResult, FileChooserError> {
        info!("SaveFiles request");

        let session_options = SessionOptions {
            title,
            multiple: true,
            directory: true,
            save_mode: true,
            current_name: None,
            current_folder: options
                .current_folder()
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            filters: Vec::new(),
            current_filter: None,
        };

        self.run_session(FileChooserOp::SaveFiles, session_options).await
    }
}
