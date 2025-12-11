//! Submission queue types
//!
//! Queue is owned by daemon, CLI interacts via IPC.

use bincode::{Decode, Encode};

use crate::PortalType;

/// A command that can be queued for later execution
///
/// These commands are stored by the daemon and applied when a matching
/// portal session becomes active.
#[derive(Debug, Clone, Encode, Decode)]
pub enum QueuedCommand {
    /// Add URIs to the selection
    Select(Vec<String>),
    /// Remove URIs from the selection
    Deselect(Vec<String>),
    /// Clear all selected URIs
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
    fn submit_moves_pending_to_submission() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Select(vec!["file:///a".into()]));
        queue.push_command(QueuedCommand::Clear);
        queue.submit(None);

        assert!(queue.pending.is_empty(), "pending should be cleared");
        assert_eq!(queue.submissions.len(), 1);
        assert_eq!(queue.submissions[0].commands.len(), 2);
    }

    #[test]
    fn submit_preserves_portal_type() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Clear);
        queue.submit(Some(PortalType::FileChooser));

        assert_eq!(queue.submissions[0].portal, Some(PortalType::FileChooser));
    }

    #[test]
    fn submit_empty_is_noop() {
        let mut queue = SubmissionQueue::new();
        queue.submit(None);
        queue.submit(Some(PortalType::FileChooser));

        assert!(queue.submissions.is_empty());
    }

    #[test]
    fn pop_none_matches_any_portal() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Clear);
        queue.submit(None);

        let sub = queue.pop_for_portal(PortalType::FileChooser);
        assert!(sub.is_some(), "None should match any portal");
        assert!(queue.submissions.is_empty(), "pop should consume submission");
    }

    #[test]
    fn pop_specific_portal_matches_exact() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Clear);
        queue.submit(Some(PortalType::FileChooser));

        let sub = queue.pop_for_portal(PortalType::FileChooser);
        assert!(sub.is_some());
        assert!(queue.submissions.is_empty());
    }

    #[test]
    fn pop_fifo_order() {
        let mut queue = SubmissionQueue::new();

        queue.push_command(QueuedCommand::Select(vec!["first".into()]));
        queue.submit(None);

        queue.push_command(QueuedCommand::Select(vec!["second".into()]));
        queue.submit(Some(PortalType::FileChooser));

        assert_eq!(queue.submissions.len(), 2);

        // First pop gets the wildcard (None) submission
        let first = queue.pop_for_portal(PortalType::FileChooser).unwrap();
        assert!(first.portal.is_none(), "should get wildcard submission first");
        assert_eq!(queue.submissions.len(), 1);

        let second = queue.pop_for_portal(PortalType::FileChooser).unwrap();
        assert_eq!(second.portal, Some(PortalType::FileChooser));
        assert!(queue.submissions.is_empty());
    }

    #[test]
    fn commands_preserve_order() {
        let mut queue = SubmissionQueue::new();
        queue.push_command(QueuedCommand::Select(vec!["a".into()]));
        queue.push_command(QueuedCommand::Clear);
        queue.push_command(QueuedCommand::Select(vec!["b".into()]));
        queue.submit(None);

        let sub = queue.pop_for_portal(PortalType::FileChooser).unwrap();
        assert!(matches!(&sub.commands[0], QueuedCommand::Select(v) if v == &["a"]));
        assert!(matches!(&sub.commands[1], QueuedCommand::Clear));
        assert!(matches!(&sub.commands[2], QueuedCommand::Select(v) if v == &["b"]));
    }
}
