#[cfg(feature = "portal-file-chooser")]
pub mod file_chooser;
pub mod intent;
#[cfg(feature = "portal-screenshot")]
pub mod screenshot;

pub use intent::{Cardinality, Intent, IntentFamily, IntentItem, MergeOp, parse_item};

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

    /// Materialize a typed intent for this session.
    pub fn materialize_intent(&self, intent: &Intent) -> std::io::Result<Vec<String>> {
        let options = self.read_options()?;
        materialize_intent(&self.portal, &self.operation, intent, &options)
            .map_err(std::io::Error::other)
    }

    /// Portal-aware add for typed intent.
    pub fn add_intent(&self, intent: &Intent) -> std::io::Result<AddResult> {
        let entries = self.materialize_intent(intent)?;
        self.add_entries(&entries)
    }

    /// Replace the submission with materialized entries from a typed intent.
    pub fn set_intent(&self, intent: &Intent) -> std::io::Result<()> {
        let entries = self.materialize_intent(intent)?;
        files::write_lines(&self.submission_path(), &entries)
    }

    /// Remove materialized entries from the submission.
    pub fn remove_intent(&self, intent: &Intent) -> std::io::Result<()> {
        let entries = self.materialize_intent(intent)?;
        files::remove_lines(&self.submission_path(), &entries)
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

/// Materialize a typed intent into final portal submission entries.
pub fn materialize_intent(
    portal: &str,
    operation: &str,
    intent: &Intent,
    options: &serde_json::Value,
) -> Result<Vec<String>, String> {
    match portal {
        #[cfg(feature = "portal-file-chooser")]
        "file-chooser" => {
            let opts: file_chooser::SessionOptions = serde_json::from_value(options.clone())
                .map_err(|e| format!("invalid options: {e}"))?;
            file_chooser::materialize_intent(operation, intent, &opts)
        }
        #[cfg(feature = "portal-screenshot")]
        "screenshot" => screenshot::materialize_intent(operation, intent),
        _ => Err(format!(
            "unsupported portal for intent materialization: {portal}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "portal-file-chooser")]
    #[test]
    fn materialize_open_file_multi_path_intent() {
        let options = serde_json::to_value(file_chooser::SessionOptions {
            title: "Open".into(),
            mode: file_chooser::SelectionMode::Pick {
                multiple: true,
                directory: false,
            },
            current_folder: None,
            candidates: vec![],
            filters: vec![],
            current_filter: None,
        })
        .unwrap();
        let intent = Intent::multi(
            IntentFamily::Path,
            vec![
                IntentItem::Path("/tmp/a.txt".into()),
                IntentItem::Path("/tmp/b.txt".into()),
            ],
        )
        .unwrap();

        let entries = materialize_intent("file-chooser", "open-file", &intent, &options).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].starts_with("file:///"));
    }

    #[cfg(feature = "portal-screenshot")]
    #[test]
    fn materialize_pick_color_requires_single_color() {
        let intent = Intent::single(IntentItem::Color("#ff00aa".into()));
        let options = serde_json::json!({});

        let entries = materialize_intent("screenshot", "pick-color", &intent, &options).unwrap();
        assert_eq!(entries, vec!["#ff00aa"]);
    }
}
