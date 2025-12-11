use std::collections::HashMap;

use bincode::{Decode, Encode};

use super::file_chooser::SessionOptions;
use super::screenshot::ScreenshotSessionOptions;

/// Portal-specific context carried by a session
///
/// Each portal type has its own options/config. The session machinery
/// dispatches on this enum to derive environment variables, title,
/// selection behavior, etc.
#[derive(Debug, Clone, Encode, Decode)]
pub enum PortalContext {
    FileChooser(SessionOptions),
    Screenshot(ScreenshotSessionOptions),
}

impl PortalContext {
    /// Get environment variables for this portal context
    pub fn env(&self) -> HashMap<&'static str, String> {
        match self {
            PortalContext::FileChooser(opts) => opts.env(),
            PortalContext::Screenshot(opts) => opts.env(),
        }
    }

    /// Get session title (if any)
    pub fn title(&self) -> Option<&str> {
        match self {
            PortalContext::FileChooser(opts) => {
                if opts.title.is_empty() {
                    None
                } else {
                    Some(&opts.title)
                }
            }
            PortalContext::Screenshot(_) => None,
        }
    }
}
