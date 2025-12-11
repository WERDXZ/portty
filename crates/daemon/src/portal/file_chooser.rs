use std::sync::Arc;

use tracing::{debug, info, instrument};

use crate::config::Config;
use crate::session::{Session, SessionResult};
use portty_ipc::ipc::file_chooser::{Filter, FilterPattern, SessionOptions};
use portty_ipc::portal::file_chooser::{
    FileChooserError, FileChooserHandler, FileChooserResult, FileFilter, OpenFileOptions,
    SaveFileOptions, SaveFilesOptions, FilterPattern as PortalFilterPattern,
};

/// File chooser handler that spawns terminals
pub struct TtyFileChooser {
    config: Arc<Config>,
}

impl TtyFileChooser {
    pub fn new(config: Arc<Config>) -> Self {
        debug!("FileChooser initialized");
        Self { config }
    }

    fn run_session(
        &self,
        portal: &str,
        options: SessionOptions,
    ) -> Result<FileChooserResult, FileChooserError> {
        let portal_config = self.config.get_portal_config(portal);
        let exec = portal_config
            .exec
            .as_deref()
            .or(self.config.default.exec.as_deref())
            .ok_or_else(|| FileChooserError::Other("no exec configured".to_string()))?;

        debug!(exec, portal, "Creating session");

        let mut session = Session::new(
            portal,
            options,
            &self.config.builtin_path,
            &portal_config.bin,
        )
        .map_err(|e| FileChooserError::Other(format!("failed to create session: {e}")))?;

        session
            .spawn(exec, portal)
            .map_err(|e| FileChooserError::Other(format!("failed to spawn: {e}")))?;

        let result = session
            .run()
            .map_err(|e| FileChooserError::Other(format!("session failed: {e}")))?;

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
