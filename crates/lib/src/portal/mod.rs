#[cfg(feature = "portal-file-chooser")]
pub mod file_chooser;
#[cfg(feature = "portal-screenshot")]
pub mod screenshot;

use std::path::PathBuf;

use crate::files;

/// Result of adding entries to a submission
pub enum AddResult {
    /// Multi-select: appended N entries
    Appended(usize),
    /// Single-select: replaced existing entry
    Replaced,
}

/// Session context for portal-aware operations.
///
/// Auto-detects the portal type and applies smart edit behavior
/// (e.g. replace vs append) based on portal constraints.
pub struct SessionContext {
    pub portal: String,
    pub operation: String,
    session_dir: PathBuf,
}

impl SessionContext {
    /// Build from session dir. Detects portal/operation:
    /// 1. Try PORTTY_PORTAL + PORTTY_OPERATION env vars (zero I/O, always set in session terminals)
    /// 2. Fallback: read <session_dir>/portal file (for headless mode / external tools)
    pub fn from_session_dir(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let session_dir = dir.into();

        // Try env vars first (zero I/O)
        if let (Ok(portal), Ok(operation)) = (
            std::env::var("PORTTY_PORTAL"),
            std::env::var("PORTTY_OPERATION"),
        ) {
            return Ok(Self {
                portal,
                operation,
                session_dir,
            });
        }

        // Fallback: read portal file
        let portal_file = session_dir.join("portal");
        let content = std::fs::read_to_string(&portal_file)?;
        let mut lines = content.lines();
        let portal = lines.next().unwrap_or("unknown").to_string();
        let operation = lines.next().unwrap_or("unknown").to_string();

        Ok(Self {
            portal,
            operation,
            session_dir,
        })
    }

    /// Read and parse options.json
    pub fn read_options(&self) -> std::io::Result<serde_json::Value> {
        let path = self.session_dir.join("options.json");
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str(&content).map_err(std::io::Error::other)
    }

    /// Portal-aware add: auto-detects single-select -> replace
    pub fn add_entries(&self, entries: &[String]) -> std::io::Result<AddResult> {
        let sub_path = self.submission_path();
        match self.portal.as_str() {
            #[cfg(feature = "portal-file-chooser")]
            "file-chooser" => {
                let options_json = self.read_options()?;
                let opts: file_chooser::SessionOptions =
                    serde_json::from_value(options_json).map_err(std::io::Error::other)?;
                file_chooser::add_entries(&sub_path, entries, &opts)
            }
            #[cfg(feature = "portal-screenshot")]
            "screenshot" => screenshot::add_entries(&sub_path, entries),
            _ => {
                files::append_lines(&sub_path, entries)?;
                Ok(AddResult::Appended(entries.len()))
            }
        }
    }

    /// Validate current submission against portal constraints
    pub fn validate(&self) -> Result<Vec<String>, String> {
        let entries = files::read_lines(&self.submission_path());
        let options = self
            .read_options()
            .map_err(|e| format!("failed to read options: {e}"))?;
        validate(&self.portal, &self.operation, &entries, &options)
    }

    /// Path to the submission file
    pub fn submission_path(&self) -> PathBuf {
        self.session_dir.join("submission")
    }
}

/// Validate and transform a submission.
///
/// Dispatches to per-portal validate functions that both check constraints
/// and transform entries to their final form (e.g. resolving relative paths to URIs).
pub fn validate(
    portal: &str,
    operation: &str,
    entries: &[String],
    options: &serde_json::Value,
) -> Result<Vec<String>, String> {
    match portal {
        #[cfg(feature = "portal-file-chooser")]
        "file-chooser" => {
            let opts: file_chooser::SessionOptions = serde_json::from_value(options.clone())
                .map_err(|e| format!("invalid options: {e}"))?;
            file_chooser::validate(operation, entries, &opts)
        }
        #[cfg(feature = "portal-screenshot")]
        "screenshot" => screenshot::validate(operation, entries),
        _ => Ok(entries.to_vec()),
    }
}
