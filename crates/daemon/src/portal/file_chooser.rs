use std::sync::{Arc, RwLock};

use tracing::{debug, info, instrument};

use crate::config::Config;
use crate::daemon_socket::{RegisteredSession, SessionRegistry};
use crate::session::{Session, SessionResult};
use portty_ipc::ipc::file_chooser::{Filter, FilterPattern, SessionOptions};
use portty_ipc::portal::file_chooser::{
    FileChooserError, FileChooserHandler, FileChooserResult, FileFilter, OpenFileOptions,
    SaveFileOptions, SaveFilesOptions, FilterPattern as PortalFilterPattern,
};
use portty_ipc::queue::{self, QueuedCommand};

/// File chooser handler that spawns terminals
pub struct TtyFileChooser {
    config: Arc<Config>,
    registry: Arc<RwLock<SessionRegistry>>,
}

impl TtyFileChooser {
    pub fn new(config: Arc<Config>, registry: Arc<RwLock<SessionRegistry>>) -> Self {
        debug!("FileChooser initialized");
        Self { config, registry }
    }

    fn run_session(
        &self,
        portal: &str,
        options: SessionOptions,
    ) -> Result<FileChooserResult, FileChooserError> {
        // Check for queued submission first
        let mut q = queue::read_queue();
        if let Some(submission) = q.pop_for_portal(portal) {
            info!(
                portal,
                commands = submission.commands.len(),
                "Found queued submission, auto-applying"
            );

            // Save updated queue
            let _ = queue::write_queue(&q);

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

        // No queued submission - run interactive session
        let portal_config = self.config.get_portal_config(portal);

        // Get exec command - None means headless mode
        let exec = portal_config
            .exec
            .as_deref()
            .or(self.config.default.exec.as_deref())
            .filter(|s| !s.is_empty());

        let headless = exec.is_none();
        if headless {
            info!(portal, "Starting headless session (use `portty` CLI to interact)");
        } else {
            debug!(exec, portal, "Creating session");
        }

        let mut session = Session::new(
            portal,
            options,
            &self.config.builtin_path,
            &portal_config.bin,
        )
        .map_err(|e| FileChooserError::Other(format!("failed to create session: {e}")))?;

        // Register session before spawning
        let session_id = session.id().to_string();
        {
            let mut reg = self.registry.write().unwrap();
            reg.register(RegisteredSession {
                id: session_id.clone(),
                portal: session.portal().to_string(),
                title: session.title().map(String::from),
                created: session.created(),
                socket_path: session.socket_path(),
            });
        }

        // Only spawn terminal in non-headless mode
        if let Some(exec) = exec {
            session
                .spawn(exec, portal)
                .map_err(|e| FileChooserError::Other(format!("failed to spawn: {e}")))?;
        }

        let result = session
            .run()
            .map_err(|e| FileChooserError::Other(format!("session failed: {e}")))?;

        // Unregister session after completion
        {
            let mut reg = self.registry.write().unwrap();
            reg.unregister(&session_id);
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
            current_filter: None, // TODO: find index
        };

        self.run_session("file-chooser", session_options)
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

        self.run_session("file-chooser", session_options)
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
            directory: true, // SaveFiles selects a directory
            save_mode: true,
            current_name: None,
            current_folder: options
                .current_folder()
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            filters: Vec::new(),
            current_filter: None,
        };

        self.run_session("file-chooser", session_options)
    }
}
