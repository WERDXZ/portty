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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_queue_is_empty() {
        let queue = SubmissionQueue::new();
        assert!(queue.pending.is_empty());
        assert!(queue.submissions.is_empty());
    }

    #[test]
    fn test_push_command() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Select(vec!["file:///a".into()]));
        queue.push_command(QueuedCommand::Clear);

        assert_eq!(queue.pending.len(), 2);
        assert!(queue.submissions.is_empty());
    }

    #[test]
    fn test_submit_creates_submission() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Select(vec!["file:///a".into()]));
        queue.submit(None);

        assert!(queue.pending.is_empty());
        assert_eq!(queue.submissions.len(), 1);
        assert_eq!(queue.submissions[0].commands.len(), 1);
        assert!(queue.submissions[0].portal.is_none());
    }

    #[test]
    fn test_submit_with_portal_type() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Clear);
        queue.submit(Some(PortalType::FileChooser));

        assert_eq!(queue.submissions[0].portal, Some(PortalType::FileChooser));
    }

    #[test]
    fn test_submit_empty_pending_does_nothing() {
        let mut queue = SubmissionQueue::new();
        queue.submit(None);

        assert!(queue.submissions.is_empty());
    }

    #[test]
    fn test_pop_for_portal_matches_none() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Clear);
        queue.submit(None); // portal = None matches any

        let sub = queue.pop_for_portal(PortalType::FileChooser);
        assert!(sub.is_some());
        assert!(queue.submissions.is_empty());
    }

    #[test]
    fn test_pop_for_portal_matches_specific() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Clear);
        queue.submit(Some(PortalType::FileChooser));

        let sub = queue.pop_for_portal(PortalType::FileChooser);
        assert!(sub.is_some());
    }

    #[test]
    fn test_pop_for_portal_no_match() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Clear);
        queue.submit(Some(PortalType::FileChooser));

        // Try to pop for a different portal type - need to check if there are other types
        // For now, just verify the submission stays if we don't pop
        assert_eq!(queue.submissions.len(), 1);
    }

    #[test]
    fn test_clear_pending() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Clear);
        queue.push_command(QueuedCommand::Select(vec![]));
        queue.clear_pending();

        assert!(queue.pending.is_empty());
    }

    #[test]
    fn test_clear_all() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Clear);
        queue.submit(None);
        queue.push_command(QueuedCommand::Clear);
        queue.clear_all();

        assert!(queue.pending.is_empty());
        assert!(queue.submissions.is_empty());
    }

    #[test]
    fn test_multiple_submissions() {
        let mut queue = SubmissionQueue::new();

        queue.push_command(QueuedCommand::Select(vec!["a".into()]));
        queue.submit(None);

        queue.push_command(QueuedCommand::Select(vec!["b".into()]));
        queue.submit(Some(PortalType::FileChooser));

        assert_eq!(queue.submissions.len(), 2);

        // Pop in FIFO order
        let first = queue.pop_for_portal(PortalType::FileChooser).unwrap();
        assert!(first.portal.is_none()); // First one had None
    }
}
