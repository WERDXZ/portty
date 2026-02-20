use std::fmt::Display;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::files;

use super::AddResult;

/// How the file chooser session operates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionMode {
    Pick { multiple: bool, directory: bool },
    Save,
    SaveMultiple,
}

impl Default for SelectionMode {
    fn default() -> Self {
        Self::Pick {
            multiple: false,
            directory: false,
        }
    }
}

impl Display for SelectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pick {
                multiple: false,
                directory: false,
            } => write!(f, "Pick"),
            Self::Pick {
                multiple: true,
                directory: false,
            } => write!(f, "Pick (multiple)"),
            Self::Pick {
                multiple: false,
                directory: true,
            } => write!(f, "Pick (directory)"),
            Self::Pick {
                multiple: true,
                directory: true,
            } => write!(f, "Pick (multiple, directory)"),
            Self::Save => write!(f, "Save"),
            Self::SaveMultiple => write!(f, "SaveMultiple"),
        }
    }
}

/// File filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub name: String,
    pub patterns: Vec<FilterPattern>,
}

/// Filter pattern type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterPattern {
    Glob(String),
    MimeType(String),
}

/// Session options for file chooser
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionOptions {
    pub title: String,
    pub mode: SelectionMode,
    pub current_folder: Option<String>,
    pub candidates: Vec<String>,
    pub filters: Vec<Filter>,
    pub current_filter: Option<usize>,
}

/// Validate and transform file chooser submission entries into file:// URIs.
///
/// Resolves relative paths against `current_folder` from options.
/// For save-files with candidates, builds URIs from selected folder + candidate filenames.
pub fn validate(
    operation: &str,
    entries: &[String],
    options: &SessionOptions,
) -> Result<Vec<String>, String> {
    if entries.is_empty() {
        return Err("No entries in submission".to_string());
    }

    let current_folder = options.current_folder.as_deref().map(Path::new);

    match operation {
        "save-file" => {
            if entries.len() > 1 {
                return Err(format!("Save mode expects 1 entry, got {}", entries.len()));
            }
            let candidate_name = options.candidates.first().map(String::as_str);
            Ok(entries
                .iter()
                .map(|e| resolve_save_file_to_uri(e, current_folder, candidate_name))
                .collect())
        }
        "save-files" => {
            if options.candidates.is_empty() {
                return Ok(entries
                    .iter()
                    .map(|e| resolve_to_uri(e, current_folder))
                    .collect());
            }

            // User selected a folder; build URIs from folder + candidate filenames
            let folder_entry = entries.first().ok_or("No folder selected for save-files")?;
            let folder = resolve_path(folder_entry, current_folder);
            let folder = if folder.is_file() {
                folder.parent().unwrap_or(&folder).to_path_buf()
            } else {
                folder
            };
            Ok(options
                .candidates
                .iter()
                .map(|name| path_to_file_uri(&folder.join(name)))
                .collect())
        }
        "open-file" => {
            if let SelectionMode::Pick { multiple, .. } = options.mode
                && !multiple
                && entries.len() > 1
            {
                return Err(format!(
                    "Single-pick mode expects 1 entry, got {}",
                    entries.len()
                ));
            }
            Ok(entries
                .iter()
                .map(|e| resolve_to_uri(e, current_folder))
                .collect())
        }
        _ => Ok(entries
            .iter()
            .map(|e| resolve_to_uri(e, current_folder))
            .collect()),
    }
}

/// Resolve a save-file entry to a file:// URI.
///
/// If the selected path is a directory and a candidate filename exists,
/// append that filename to produce the final target file path.
fn resolve_save_file_to_uri(
    entry: &str,
    current_folder: Option<&Path>,
    candidate_name: Option<&str>,
) -> String {
    let selected = resolve_path(entry, current_folder);

    if let Some(name) = candidate_name && !name.is_empty() && selected.is_dir() {
        return path_to_file_uri(&selected.join(name));
    }

    path_to_file_uri(&selected)
}

/// Resolve an entry string to an absolute path, using current_folder for relative paths.
fn resolve_path(entry: &str, current_folder: Option<&Path>) -> PathBuf {
    let path_str = entry.strip_prefix("file://").unwrap_or(entry);
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(folder) = current_folder {
        folder.join(path)
    } else {
        path.to_path_buf()
    }
}

/// Convert an absolute path to a file:// URI with proper percent-encoding.
fn path_to_file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map(|u| u.to_string())
        .unwrap_or_else(|()| format!("file://{}", path.display()))
}

/// Resolve an entry to a file:// URI, resolving relative paths against current_folder.
fn resolve_to_uri(entry: &str, current_folder: Option<&Path>) -> String {
    let path = resolve_path(entry, current_folder);
    path_to_file_uri(&path)
}

/// Resolve relative paths against CWD so entries are stable regardless of
/// later directory changes. Entries that are already absolute or file:// URIs
/// pass through unchanged.
fn resolve_entries_to_absolute(entries: &[String]) -> Vec<String> {
    let cwd = std::env::current_dir().ok();
    entries
        .iter()
        .map(|e| {
            let (prefix, path_str) = if let Some(rest) = e.strip_prefix("file://") {
                ("file://", rest)
            } else {
                ("", e.as_str())
            };
            let path = Path::new(path_str);
            if path.is_absolute() {
                e.clone()
            } else if let Some(ref cwd) = cwd {
                format!("{}{}", prefix, cwd.join(path).display())
            } else {
                e.clone()
            }
        })
        .collect()
}

/// Smart add entries: respects single/multi-select constraints.
///
/// Resolves relative paths against CWD at edit time.
/// In multi-pick mode, appends entries. In all other modes (single-pick, save, save-multiple),
/// replaces the submission.
pub fn add_entries(
    sub_path: &Path,
    entries: &[String],
    options: &SessionOptions,
) -> std::io::Result<AddResult> {
    let resolved = resolve_entries_to_absolute(entries);
    let is_multi = matches!(options.mode, SelectionMode::Pick { multiple: true, .. });
    if is_multi {
        files::append_lines(sub_path, &resolved)?;
        Ok(AddResult::Appended(resolved.len()))
    } else {
        files::write_lines(sub_path, &resolved)?;
        Ok(AddResult::Replaced)
    }
}
