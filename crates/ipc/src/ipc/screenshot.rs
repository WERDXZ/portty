use std::collections::HashMap;
use std::fmt::Display;

use bincode::{Decode, Encode};

/// Screenshot operation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum ScreenshotMode {
    /// Take a screenshot (optionally interactive)
    Screenshot { interactive: bool },
    /// Pick a color from the screen
    PickColor,
}

impl Display for ScreenshotMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Screenshot { interactive: true } => write!(f, "Screenshot (interactive)"),
            Self::Screenshot { interactive: false } => write!(f, "Screenshot"),
            Self::PickColor => write!(f, "PickColor"),
        }
    }
}

/// Session options for screenshot portal
#[derive(Debug, Clone, Encode, Decode)]
pub struct ScreenshotSessionOptions {
    /// Operation mode
    pub mode: ScreenshotMode,
    /// Application ID requesting the screenshot
    pub app_id: String,
    /// Whether the dialog should be modal
    pub modal: bool,
}

impl ScreenshotSessionOptions {
    pub fn env(&self) -> HashMap<&'static str, String> {
        let mut map = HashMap::new();

        let mode_str = match self.mode {
            ScreenshotMode::Screenshot { .. } => "Screenshot",
            ScreenshotMode::PickColor => "PickColor",
        };
        map.insert("PORTTY_MODE", mode_str.to_string());

        if let ScreenshotMode::Screenshot { interactive } = self.mode {
            map.insert("PORTTY_INTERACTIVE", interactive.to_string());
        }

        map.insert("PORTTY_APP_ID", self.app_id.clone());
        map.insert("PORTTY_MODAL", self.modal.to_string());

        map
    }
}
