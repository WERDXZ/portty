use std::{collections::HashMap, fmt::Display};

use bincode::{Decode, Encode};

/// Session options sent to builtins
#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct SessionOptions {
    /// Dialog title
    pub title: String,

    /// Allow multiple selection
    pub multiple: bool,

    /// Select directories instead of files
    pub directory: bool,

    /// Save mode (for SaveFile/SaveFiles)
    pub save_mode: bool,

    /// Suggested filename (for SaveFile)
    pub current_name: Option<String>,

    /// Current folder path
    pub current_folder: Option<String>,

    /// Proposed filenames for SaveFiles (empty for other operations)
    /// When non-empty, indicates SaveFiles mode where user picks a folder
    /// and these filenames are appended to create the final URIs
    pub files: Vec<String>,

    /// File filters
    pub filters: Vec<Filter>,

    /// Currently active filter index
    pub current_filter: Option<usize>,
}

impl SessionOptions {
    pub fn env(&self) -> HashMap<&'static str, String>{
        let mut hashmap = HashMap::new();
        hashmap.insert("PORTTY-TITLE", self.title.clone());

        if let Some(cwd) = &self.current_folder {
            hashmap.insert("PORTTY-DEFAULT-CWD", cwd.clone());
        }

        if let Some(file) = &self.current_name {
            hashmap.insert("PORTTY-DEFAULT-FILE", file.clone());
        }

        hashmap
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
            Self::MimeType(s) => ("Mime", s)
        };
        write!(f, "{k} - {v}")
    }
}
