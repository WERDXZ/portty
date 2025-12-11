//! Portal type enum

use bincode::{Decode, Encode};
use std::str::FromStr;

/// Supported portal types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
#[non_exhaustive]
pub enum PortalType {
    FileChooser,
    Screenshot,
}

impl PortalType {
    /// Get the string identifier used in config and D-Bus
    pub fn as_str(&self) -> &'static str {
        match self {
            PortalType::FileChooser => "file-chooser",
            PortalType::Screenshot => "screenshot",
        }
    }
}

/// Error returned when parsing an unknown portal type string
#[derive(Debug, Clone)]
pub struct ParsePortalTypeError(String);

impl std::fmt::Display for ParsePortalTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown portal type: {}", self.0)
    }
}

impl std::error::Error for ParsePortalTypeError {}

impl FromStr for PortalType {
    type Err = ParsePortalTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "file-chooser" => Ok(PortalType::FileChooser),
            "screenshot" => Ok(PortalType::Screenshot),
            _ => Err(ParsePortalTypeError(s.to_string())),
        }
    }
}

impl std::fmt::Display for PortalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
