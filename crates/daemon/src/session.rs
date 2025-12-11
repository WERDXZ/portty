use std::collections::{HashMap, HashSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use portty_ipc::ipc::file_chooser::SessionOptions;
use portty_ipc::ipc::{read_message, write_message};
use portty_ipc::queue::QueuedCommand;
use portty_ipc::{SessionRequest, SessionResponse};

/// Default command shims for each portal type
/// Returns (shim_name, portty_subcommand) pairs
fn default_commands(portal: &str) -> &'static [(&'static str, &'static str)] {
    match portal {
        // "sel" shim avoids conflict with POSIX `select` builtin
        "file-chooser" => &[("sel", "select"), ("submit", "submit"), ("cancel", "cancel")],
        _ => &[],
    }
}

/// Unique session identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(String);

impl SessionId {
    /// Create a new unique session ID based on current timestamp
    pub fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self(format!("{:x}", ts))
    }

    /// Get the session ID as a string slice
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
///
/// Represents an active file chooser or other portal dialog. Each session:
/// - Has a unique ID and temporary directory under `/tmp/portty/<uid>/`
/// - Creates shell shims in `bin/` for CLI commands (sel, submit, cancel)
/// - Listens on a Unix socket for IPC commands
/// - Optionally spawns a terminal process for interactive use
///
/// Sessions can run in two modes:
/// - **Terminal mode**: Spawns a terminal with environment set up
/// - **Headless mode**: No terminal, controlled entirely via CLI/socket
pub struct Session {
    id: SessionId,
    dir: PathBuf,
    /// Cached socket path to avoid repeated PathBuf::join() calls
    socket_path: PathBuf,
    child: Option<Child>,
    listener: Option<UnixListener>,
    options: SessionOptions,
    /// Selected URIs (HashSet for O(1) lookups and automatic deduplication)
    selection: HashSet<String>,
    created: u64,
}

impl Session {
    /// Create a new session with its directory
    pub fn new(
        portal: &str,
        options: SessionOptions,
        custom_bins: &HashMap<String, String>,
    ) -> std::io::Result<Self> {
        let id = SessionId::new();
        let dir = session_dir(&id);

        // Create session directory
        fs::create_dir_all(&dir)?;

        // Write portal type
        fs::write(dir.join("portal"), portal)?;

        // Create bin directory with shims
        let bin_dir = dir.join("bin");
        fs::create_dir_all(&bin_dir)?;

        // Create default command shims (call portty CLI)
        for (shim_name, subcommand) in default_commands(portal) {
            // Skip if overridden by custom bin
            if custom_bins.contains_key(*shim_name) {
                continue;
            }
            let shim_path = bin_dir.join(shim_name);
            // portty auto-detects session mode via PORTTY_SOCK env var
            let shim_content = format!(
                "#!/bin/sh\nexec portty {} \"$@\"\n",
                subcommand
            );
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

        // Create Unix socket
        let sock_path = dir.join("sock");
        let listener = UnixListener::bind(&sock_path)?;

        // Get creation timestamp
        use std::time::{SystemTime, UNIX_EPOCH};
        let created = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(Self {
            id,
            dir,
            socket_path: sock_path,
            child: None,
            listener: Some(listener),
            options,
            selection: HashSet::new(),
            created,
        })
    }

    /// Get session ID
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    /// Get session title (from options)
    pub fn title(&self) -> Option<&str> {
        if self.options.title.is_empty() {
            None
        } else {
            Some(&self.options.title)
        }
    }

    /// Get creation timestamp
    pub fn created(&self) -> u64 {
        self.created
    }

    /// Get socket path (cached)
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Apply pending commands to pre-populate the selection
    ///
    /// This allows incomplete queue commands to be transferred to an
    /// interactive session, letting the user review before submitting.
    pub fn apply_pending(&mut self, commands: Vec<QueuedCommand>) {
        for cmd in commands {
            match cmd {
                QueuedCommand::Select(uris) => {
                    self.selection.extend(uris);
                }
                QueuedCommand::Deselect(uris) => {
                    for uri in uris {
                        self.selection.remove(&uri);
                    }
                }
                QueuedCommand::Clear => {
                    self.selection.clear();
                }
            }
        }
    }

    /// Spawn a terminal with the given exec command
    pub fn spawn(&mut self, exec: &str, portal: &str) -> std::io::Result<()> {
        // Parse exec command (simple shell-like splitting)
        let parts: Vec<&str> = exec.split_whitespace().collect();
        if parts.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "empty exec command",
            ));
        }

        let (program, args) = parts.split_first().unwrap();

        // Build environment
        let mut cmd = Command::new(program);
        cmd.args(args);

        let bin_dir = self.dir.join("bin");

        // Inject our environment variables
        cmd.env("PORTTY_SESSION", self.id.as_str());
        cmd.env("PORTTY_PORTAL", portal);
        cmd.env("PORTTY_DIR", &self.dir);
        cmd.env("PORTTY_SOCK", self.dir.join("sock"));

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

    /// Run the session, handling IPC and waiting for completion
    ///
    /// In terminal mode (child process exists): exits when terminal closes
    /// In headless mode (no child): waits for explicit Submit/Cancel via IPC
    pub fn run(&mut self) -> std::io::Result<SessionResult> {
        let listener = self.listener.take().ok_or_else(|| {
            std::io::Error::other("no listener")
        })?;

        // Set socket to non-blocking for polling
        listener.set_nonblocking(true)?;

        let mut cancelled = false;
        let mut submitted = false;
        let headless = self.child.is_none();

        loop {
            // Check if child is still running (only in terminal mode)
            if let Some(ref mut child) = self.child {
                match child.try_wait()? {
                    Some(_status) => {
                        // Child exited - treat as implicit submit
                        submitted = true;
                        break;
                    }
                    None => {
                        // Still running, check for IPC
                    }
                }
            }

            // In headless mode, exit only on explicit submit/cancel
            if headless && (submitted || cancelled) {
                break;
            }

            // Try to accept a connection
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;

                    // Handle request
                    match read_message::<SessionRequest>(&mut stream) {
                        Ok(req) => {
                            let resp = self.handle_request(req, &mut cancelled, &mut submitted);
                            let _ = write_message(&mut stream, &resp);
                        }
                        Err(e) => {
                            tracing::warn!("IPC read error: {e}");
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No connection, sleep a bit
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    tracing::warn!("Accept error: {e}");
                }
            }
        }

        // Determine result - use mem::take to avoid clone since session is consumed
        let result = if cancelled {
            SessionResult::Cancelled
        } else if submitted && !self.selection.is_empty() {
            SessionResult::Success {
                uris: std::mem::take(&mut self.selection).into_iter().collect(),
            }
        } else {
            // No selection or not submitted properly
            SessionResult::Cancelled
        };

        Ok(result)
    }

    fn handle_request(
        &mut self,
        req: SessionRequest,
        cancelled: &mut bool,
        submitted: &mut bool,
    ) -> SessionResponse {
        use portty_ipc::{Request, Response};

        match req {
            Request::GetOptions => Response::Options(self.options.clone()),
            Request::GetSelection => {
                // Convert HashSet to Vec for response
                Response::Selection(self.selection.iter().cloned().collect())
            }
            Request::Select(uris) => {
                // HashSet handles deduplication automatically
                self.selection.extend(uris);
                Response::Ok
            }
            Request::Deselect(uris) => {
                for uri in uris {
                    self.selection.remove(&uri);
                }
                Response::Ok
            }
            Request::Clear => {
                self.selection.clear();
                Response::Ok
            }
            Request::Submit => {
                *submitted = true;
                // Kill the child process if running (will trigger loop exit)
                if let Some(ref mut child) = self.child {
                    let _ = child.kill();
                }
                Response::Ok
            }
            Request::Cancel => {
                *cancelled = true;
                // Kill the child process if running
                if let Some(ref mut child) = self.child {
                    let _ = child.kill();
                }
                Response::Ok
            }
            // NoExtension is uninhabited - this arm is unreachable
            Request::Extended(never) => match never {},
        }
    }

    /// Clean up session directory
    pub fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Result from a session
#[derive(Debug)]
pub enum SessionResult {
    Success { uris: Vec<String> },
    Cancelled,
}

/// Get the base directory for sessions (/tmp/portty/<uid>/)
pub fn base_dir() -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    // Get UID from /proc/self metadata
    let uid = std::fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(0);
    PathBuf::from(format!("/tmp/portty/{}", uid))
}

/// Get directory for a specific session
fn session_dir(id: &SessionId) -> PathBuf {
    base_dir().join(id.as_str())
}
