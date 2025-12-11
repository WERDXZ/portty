use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use libportty::{files, paths};
use tracing::info;

/// Default command shims — same for all portals
const DEFAULT_SHIMS: &[(&str, &str)] = &[
    ("sel", "edit"),
    ("desel", "edit --remove"),
    ("reset", "edit --reset"),
    ("submit", "submit"),
    ("cancel", "cancel"),
    ("info", "info"),
];

/// Signal sent to the session thread
pub enum SessionSignal {
    Submit,
    Cancel,
    ChildExited,
}

/// Control handle held by the daemon to signal a session
pub struct SessionControl {
    sender: mpsc::Sender<SessionSignal>,
}

impl SessionControl {
    pub fn new(sender: mpsc::Sender<SessionSignal>) -> Self {
        Self { sender }
    }

    pub fn submit(&self) {
        let _ = self.sender.send(SessionSignal::Submit);
    }

    pub fn cancel(&self) {
        let _ = self.sender.send(SessionSignal::Cancel);
    }
}

/// Monotonic counter to guarantee unique session IDs even within the same nanosecond
static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Unique session identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let seq = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(format!("{:x}-{:x}", ts, seq))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A running portal session
pub struct Session {
    id: SessionId,
    dir: PathBuf,
    child: Option<Child>,
    sender: mpsc::Sender<SessionSignal>,
    receiver: mpsc::Receiver<SessionSignal>,
    created: u64,
}

impl Session {
    /// Create a new session with its directory and file-based state.
    /// Use `SessionRegistry::create_session` instead of calling this directly.
    pub(crate) fn new(
        portal: &str,
        operation: &str,
        options: &serde_json::Value,
        initial_entries: &[String],
        custom_bins: &HashMap<String, String>,
        sender: mpsc::Sender<SessionSignal>,
        receiver: mpsc::Receiver<SessionSignal>,
    ) -> std::io::Result<Self> {
        let id = SessionId::new();
        paths::ensure_base_dir()?;
        let dir = paths::base_dir().join(id.as_str());

        // Create session directory
        fs::create_dir_all(&dir)?;

        // Write portal type
        fs::write(dir.join("portal"), format!("{}\n{}", portal, operation))?;

        // Write options.json
        let options_json = serde_json::to_string_pretty(options).map_err(std::io::Error::other)?;
        fs::write(dir.join("options.json"), options_json)?;

        // Build initial submission
        let submission_content = if initial_entries.is_empty() {
            String::new()
        } else {
            format!("{}\n", initial_entries.join("\n"))
        };
        fs::write(dir.join("submission"), &submission_content)?;

        // Create bin directory with shims
        let bin_dir = dir.join("bin");
        fs::create_dir_all(&bin_dir)?;

        for (shim_name, subcommand) in DEFAULT_SHIMS {
            if custom_bins.contains_key(*shim_name) {
                continue;
            }
            let shim_path = bin_dir.join(shim_name);
            let shim_content = format!("#!/bin/sh\nexec portty {} \"$@\"\n", subcommand);
            fs::write(&shim_path, shim_content)?;
            fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755))?;
        }

        // Create custom bin shims
        for (name, command) in custom_bins {
            let shim_path = bin_dir.join(name);
            let shim_content = format!("#!/bin/sh\n{}\n", command);
            fs::write(&shim_path, shim_content)?;
            fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755))?;
        }

        // Get creation timestamp
        use std::time::{SystemTime, UNIX_EPOCH};
        let created = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Self {
            id,
            dir,
            child: None,
            sender,
            receiver,
            created,
        })
    }

    pub fn id(&self) -> &SessionId {
        &self.id
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn created(&self) -> u64 {
        self.created
    }

    /// Spawn a terminal with the given exec command
    pub fn spawn(&mut self, exec: &str, portal: &str, operation: &str) -> std::io::Result<()> {
        use std::os::linux::process::CommandExt as _;

        let parts: Vec<&str> = exec.split_whitespace().collect();
        if parts.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "empty exec command",
            ));
        }

        let (program, args) = parts.split_first().unwrap();

        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.create_pidfd(true);

        let bin_dir = self.dir.join("bin");

        // Set universal env vars
        cmd.env("PORTTY_SESSION", self.id.as_str());
        cmd.env("PORTTY_DIR", &self.dir);
        cmd.env("PORTTY_PORTAL", portal);
        cmd.env("PORTTY_OPERATION", operation);

        // Prepend session bin dir to PATH
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", format!("{}:{}", bin_dir.display(), path));
        } else {
            cmd.env("PATH", bin_dir);
        }

        let child = cmd.spawn()?;
        self.child = Some(child);

        Ok(())
    }

    /// Run the session, waiting for child exit or control signals.
    ///
    /// Converts the child process into a `PidFd` shared between a monitor
    /// thread (that waits for exit) and this thread (that can kill on
    /// submit/cancel). The channel `recv()` blocks cleanly with no polling.
    pub fn run(&mut self) -> std::io::Result<SessionResult> {
        use std::os::linux::process::ChildExt as _;

        let pidfd = if let Some(child) = self.child.take() {
            let pidfd = Arc::new(child.into_pidfd().map_err(|_child| {
                std::io::Error::new(std::io::ErrorKind::Unsupported, "pidfd not available")
            })?);

            let monitor_pidfd = Arc::clone(&pidfd);
            let sender = self.sender.clone();
            std::thread::spawn(move || {
                let _ = monitor_pidfd.wait();
                let _ = sender.send(SessionSignal::ChildExited);
            });

            Some(pidfd)
        } else {
            None
        };

        match self.receiver.recv() {
            Ok(SessionSignal::Submit) => {
                if let Some(ref pidfd) = pidfd {
                    let _ = pidfd.kill();
                    let _ = pidfd.wait();
                }
                self.read_result()
            }
            Ok(SessionSignal::Cancel) => {
                if let Some(ref pidfd) = pidfd {
                    let _ = pidfd.kill();
                    let _ = pidfd.wait();
                }
                Ok(SessionResult::Cancelled)
            }
            Ok(SessionSignal::ChildExited) => self.read_result(),
            Err(_) => {
                // All senders dropped — session is orphaned
                if let Some(ref pidfd) = pidfd {
                    let _ = pidfd.kill();
                    let _ = pidfd.wait();
                }
                Ok(SessionResult::Cancelled)
            }
        }
    }

    fn read_result(&self) -> std::io::Result<SessionResult> {
        let entries = files::read_lines(&self.dir.join("submission"));
        if entries.is_empty() {
            Ok(SessionResult::Cancelled)
        } else {
            Ok(SessionResult::Success { entries })
        }
    }

    pub fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Drain pending/submission into the session's submission file, then truncate pending.
/// Only truncates pending after a successful write to avoid data loss.
pub fn drain_pending_to(session_dir: &Path) {
    let pending_sub = paths::pending_dir().join("submission");
    let content = fs::read_to_string(&pending_sub).unwrap_or_default();
    if content.trim().is_empty() {
        return;
    }

    let session_sub = session_dir.join("submission");
    let written = fs::OpenOptions::new()
        .append(true)
        .open(&session_sub)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(content.as_bytes())?;
            f.flush()
        });

    match written {
        Ok(()) => {
            let _ = fs::write(&pending_sub, "");
            let count = content.lines().filter(|l| !l.is_empty()).count();
            info!(entries = count, "Drained pending entries to session");
        }
        Err(e) => {
            tracing::warn!("Failed to drain pending to session, preserving pending: {e}");
        }
    }
}

/// Pop a queued submission from the submissions directory matching the portal type.
///
/// Submission dirs are named `<timestamp>-<portal>`. Scans in FIFO order.
pub fn pop_queued_submission(portal: &str) -> Option<Vec<String>> {
    let subs_dir = paths::submissions_dir();
    let mut entries: Vec<_> = fs::read_dir(&subs_dir)
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;

    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let sub_dir = entry.path();
        if !sub_dir.is_dir() {
            continue;
        }

        let dir_name = entry.file_name();
        let dir_name = dir_name.to_string_lossy();
        let dir_portal = dir_name.split_once('-').map(|(_, p)| p).unwrap_or("any");

        if dir_portal == "any" || dir_portal == portal {
            let submission = files::read_lines(&sub_dir.join("submission"));
            let _ = fs::remove_dir_all(&sub_dir);
            return Some(submission);
        }
    }

    None
}

/// Result from a session
#[derive(Debug)]
pub enum SessionResult {
    Success { entries: Vec<String> },
    Cancelled,
}
