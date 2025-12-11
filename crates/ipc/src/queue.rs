//! Submission queue for pre-queued portal commands
//!
//! Allows users to queue file selections before dialogs open.
//! When a session spawns, it pops a submission from the queue.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A command that can be queued
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueuedCommand {
    Select(Vec<String>),
    Deselect(Vec<String>),
    Clear,
}

/// A complete submission ready to be applied to a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Submission {
    /// Commands to execute
    pub commands: Vec<QueuedCommand>,
    /// Unix timestamp when created
    pub created: u64,
    /// Optional portal type filter (e.g., "file-chooser")
    /// If None, matches any portal
    pub portal: Option<String>,
}

/// The submission queue
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubmissionQueue {
    /// Pending commands not yet submitted
    pub pending: Vec<QueuedCommand>,
    /// Completed submissions waiting for sessions
    pub submissions: Vec<Submission>,
}

impl SubmissionQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a command to pending
    pub fn push_command(&mut self, cmd: QueuedCommand) {
        self.pending.push(cmd);
    }

    /// Create a submission from pending commands
    pub fn submit(&mut self, portal: Option<String>) {
        if self.pending.is_empty() {
            return;
        }

        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let submission = Submission {
            commands: std::mem::take(&mut self.pending),
            created,
            portal,
        };

        self.submissions.push(submission);
    }

    /// Pop a submission matching the given portal type
    pub fn pop_for_portal(&mut self, portal: &str) -> Option<Submission> {
        // Find first matching submission (None matches any portal)
        let idx = self.submissions.iter().position(|s| {
            s.portal.as_ref().map_or(true, |p| p == portal)
        })?;

        Some(self.submissions.remove(idx))
    }

    /// Clear pending commands
    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }

    /// Clear everything
    pub fn clear_all(&mut self) {
        self.pending.clear();
        self.submissions.clear();
    }
}

/// Get the queue file path
pub fn queue_path() -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let uid = fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(0);
    PathBuf::from(format!("/tmp/portty/{}/queue.bin", uid))
}

/// Read queue from file
pub fn read_queue() -> SubmissionQueue {
    let path = queue_path();
    match fs::read(&path) {
        Ok(data) => bincode::deserialize(&data).unwrap_or_default(),
        Err(_) => SubmissionQueue::new(),
    }
}

/// Write queue to file
pub fn write_queue(queue: &SubmissionQueue) -> std::io::Result<()> {
    let path = queue_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let data = bincode::serialize(queue)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    fs::write(&path, data)
}

/// Clear queue file
pub fn clear_queue() -> std::io::Result<()> {
    let path = queue_path();
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}
