//! Portal type enum

use serde::{Deserialize, Serialize};

/// Supported portal types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PortalType {
    FileChooser,
    // Future portals:
    // Screenshot,
    // Print,
    // Notification,
    // etc.
}

impl PortalType {
    /// Get the string identifier used in config and D-Bus
    pub fn as_str(&self) -> &'static str {
        match self {
            PortalType::FileChooser => "file-chooser",
        }
    }

    /// Parse from string identifier
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "file-chooser" => Some(PortalType::FileChooser),
            _ => None,
        }
    }
}

impl std::fmt::Display for PortalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
