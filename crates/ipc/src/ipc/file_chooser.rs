use std::{collections::HashMap, fmt::Display};

use bincode::{Decode, Encode};

/// How the file chooser session operates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum SelectionMode {
    /// Browse and pick files/directories
    Pick { multiple: bool, directory: bool },
    /// Save a single file (suggest one filename)
    Save,
    /// Save multiple files (app provides filenames, user picks folder)
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

/// Session options sent to builtins
#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct SessionOptions {
    /// Dialog title
    pub title: String,

    /// Selection mode (replaces multiple, directory, save_mode)
    pub mode: SelectionMode,

    /// Current folder path
    pub current_folder: Option<String>,

    /// Candidate filenames: suggested name for Save, file list for SaveMultiple, empty for Pick
    pub candidates: Vec<String>,

    /// File filters
    pub filters: Vec<Filter>,

    /// Currently active filter index
    pub current_filter: Option<usize>,
}

impl SessionOptions {
    pub fn env(&self) -> HashMap<&'static str, String> {
        let mut map = HashMap::new();

        map.insert("PORTTY_TITLE", self.title.clone());

        let (mode_str, multiple, directory) = match self.mode {
            SelectionMode::Pick {
                multiple,
                directory,
            } => ("Pick", multiple, directory),
            SelectionMode::Save => ("Save", false, false),
            SelectionMode::SaveMultiple => ("SaveMultiple", false, false),
        };

        map.insert("PORTTY_MODE", mode_str.to_string());
        map.insert("PORTTY_MULTIPLE", multiple.to_string());
        map.insert("PORTTY_DIRECTORY", directory.to_string());

        if let Some(ref folder) = self.current_folder {
            map.insert("PORTTY_FOLDER", folder.clone());
        }

        if !self.candidates.is_empty() {
            map.insert("PORTTY_CANDIDATES", self.candidates.join("\n"));
        }

        if !self.filters.is_empty() {
            let formatted: Vec<String> = self
                .filters
                .iter()
                .map(|f| {
                    let patterns: Vec<String> =
                        f.patterns.iter().map(|p| p.to_string()).collect();
                    format!("{}: {}", f.name, patterns.join(", "))
                })
                .collect();
            map.insert("PORTTY_FILTERS", formatted.join("\n"));
        }

        map
    }
}

/// File filter
#[derive(Debug, Clone, Encode, Decode)]
pub struct Filter {
    pub name: String,
    pub patterns: Vec<FilterPattern>,
}

/// Filter pattern type
#[derive(Debug, Clone, Encode, Decode)]
pub enum FilterPattern {
    Glob(String),
    MimeType(String),
}

impl Display for FilterPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (k, v) = match &self {
            Self::Glob(s) => ("Glob", s),
            Self::MimeType(s) => ("Mime", s),
        };
        write!(f, "{k} - {v}")
    }
}
