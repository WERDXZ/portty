//! Submission queue types
//!
//! Queue is owned by daemon, CLI interacts via IPC.

use bincode::{Decode, Encode};

use crate::PortalType;

/// A command that can be queued
#[derive(Debug, Clone, Encode, Decode)]
pub enum QueuedCommand {
    Select(Vec<String>),
    Deselect(Vec<String>),
    Clear,
}

/// A complete submission ready to be applied to a session
#[derive(Debug, Clone, Encode, Decode)]
pub struct Submission {
    /// Commands to execute
    pub commands: Vec<QueuedCommand>,
    /// Unix timestamp when created
    pub created: u64,
    /// Target portal type (None = any)
    pub portal: Option<PortalType>,
}

/// The submission queue (owned by daemon)
#[derive(Debug, Clone, Default)]
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
    pub fn submit(&mut self, portal: Option<PortalType>) {
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
    pub fn pop_for_portal(&mut self, portal: PortalType) -> Option<Submission> {
        // Find first matching submission (None matches any portal)
        let idx = self.submissions.iter().position(|s| {
            s.portal.is_none_or(|p| p == portal)
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

    /// Iterate over pending commands
    pub fn pending_iter(&self) -> impl Iterator<Item = &QueuedCommand> {
        self.pending.iter()
    }

    /// Iterate over submissions
    pub fn submissions_iter(&self) -> impl Iterator<Item = &Submission> {
        self.submissions.iter()
    }
}
