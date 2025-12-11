use serde::{Deserialize, Serialize};

/// Request from builtin -> daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    /// Get current options (filters, title, etc.)
    GetOptions,

    /// Get current selection
    GetSelection,

    /// Add files to selection (URIs)
    Select(Vec<String>),

    /// Remove files from selection (URIs)
    Deselect(Vec<String>),

    /// Clear all selection
    Clear,

    /// Submit/confirm the selection
    Submit,

    /// Cancel the operation
    Cancel,
}

/// Response from daemon -> builtin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    /// Session options
    Options(SessionOptions),

    /// Current selection
    Selection(Vec<String>),

    /// Operation completed successfully
    Ok,

    /// Error occurred
    Error(String),
}

/// Session options sent to builtins
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

    /// File filters
    pub filters: Vec<Filter>,

    /// Currently active filter index
    pub current_filter: Option<usize>,
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
