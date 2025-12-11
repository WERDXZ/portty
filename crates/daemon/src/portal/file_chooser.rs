use std::path::Path;
use std::sync::{Arc, RwLock};

use tracing::{info, instrument};

use crate::config::Config;
use crate::daemon_socket::DaemonState;
use crate::dbus::file_chooser::{
    FileChooserError, FileChooserHandler, FileChooserResult, FileFilter,
    FilterPattern as PortalFilterPattern, OpenFileOptions, SaveFileOptions, SaveFilesOptions,
};

pub use libportty::portal::file_chooser::{Filter, FilterPattern, SelectionMode, SessionOptions};

/// Build initial submission entries from file chooser options
fn build_initial_entries(options: &SessionOptions) -> Vec<String> {
    let mut entries = Vec::new();
    if let Some(ref folder) = options.current_folder {
        match options.mode {
            SelectionMode::SaveMultiple if !options.candidates.is_empty() => {
                entries.push(format!("file://{}", folder));
            }
            SelectionMode::Save => {
                if let Some(name) = options.candidates.first() {
                    let path = Path::new(folder).join(name);
                    entries.push(format!("file://{}", path.display()));
                }
            }
            _ => {}
        }
    }
    entries
}

/// Convert a null-terminated D-Bus byte array to a String, stripping trailing nulls.
fn bytes_to_string(b: &[u8]) -> String {
    let b = if b.last() == Some(&0) {
        &b[..b.len() - 1]
    } else {
        b
    };
    String::from_utf8_lossy(b).into_owned()
}

/// Convert D-Bus filters to our filter type
fn convert_filters(filters: &[FileFilter]) -> Vec<Filter> {
    filters
        .iter()
        .map(|f| Filter {
            name: f.name().to_string(),
            patterns: f
                .patterns()
                .map(|p| match p {
                    PortalFilterPattern::Glob(s) => FilterPattern::Glob(s.to_string()),
                    PortalFilterPattern::MimeType(s) => FilterPattern::MimeType(s.to_string()),
                })
                .collect(),
        })
        .collect()
}

/// File chooser handler that spawns terminals
pub struct TtyFileChooser {
    config: Arc<Config>,
    state: Arc<RwLock<DaemonState>>,
}

impl TtyFileChooser {
    pub fn new(config: Arc<Config>, state: Arc<RwLock<DaemonState>>) -> Self {
        Self { config, state }
    }
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
            title: title.clone(),
            mode: SelectionMode::Pick {
                multiple: options.multiple().unwrap_or(false),
                directory: options.directory().unwrap_or(false),
            },
            current_folder: options.current_folder().map(bytes_to_string),
            candidates: vec![],
            filters: convert_filters(options.filters()),
            current_filter: None,
        };

        let initial_entries = build_initial_entries(&session_options);
        let options_json = serde_json::to_value(&session_options)
            .map_err(|e| FileChooserError::Other(format!("failed to serialize options: {e}")))?;

        let entries = super::run_session(
            "file-chooser",
            "open-file",
            &options_json,
            &initial_entries,
            Some(&title),
            &self.config,
            &self.state,
        )
        .await?;

        Ok(FileChooserResult::new().uris(entries))
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
            title: title.clone(),
            mode: SelectionMode::Save,
            current_folder: options.current_folder().map(bytes_to_string),
            candidates: options
                .current_name()
                .map(String::from)
                .into_iter()
                .collect(),
            filters: convert_filters(options.filters()),
            current_filter: None,
        };

        let initial_entries = build_initial_entries(&session_options);
        let options_json = serde_json::to_value(&session_options)
            .map_err(|e| FileChooserError::Other(format!("failed to serialize options: {e}")))?;

        let entries = super::run_session(
            "file-chooser",
            "save-file",
            &options_json,
            &initial_entries,
            Some(&title),
            &self.config,
            &self.state,
        )
        .await?;

        Ok(FileChooserResult::new().uris(entries))
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
        let files: Vec<String> = options.files().iter().map(|f| bytes_to_string(f)).collect();

        info!(?files, "SaveFiles request");

        let session_options = SessionOptions {
            title: title.clone(),
            mode: SelectionMode::SaveMultiple,
            current_folder: options.current_folder().map(bytes_to_string),
            candidates: files.clone(),
            filters: Vec::new(),
            current_filter: None,
        };

        let initial_entries = build_initial_entries(&session_options);
        let options_json = serde_json::to_value(&session_options)
            .map_err(|e| FileChooserError::Other(format!("failed to serialize options: {e}")))?;

        let entries = super::run_session(
            "file-chooser",
            "save-files",
            &options_json,
            &initial_entries,
            Some(&title),
            &self.config,
            &self.state,
        )
        .await?;

        Ok(FileChooserResult::new().uris(entries))
    }
}
