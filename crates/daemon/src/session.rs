use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Child, Command};

use portty_types::ipc::file_chooser::{Request, Response, SessionOptions};
use portty_types::ipc::{read_message, write_message};

/// Commands available for each portal type
fn portal_commands(portal: &str) -> &'static [&'static str] {
    match portal {
        "file-chooser" => &["select", "cancel"],
        _ => &[],
    }
}

/// Unique session identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionId(String);

impl SessionId {
    fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self(format!("{:x}", ts))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// A running session
pub struct Session {
    id: SessionId,
    dir: PathBuf,
    child: Option<Child>,
    listener: Option<UnixListener>,
    options: SessionOptions,
    selection: Vec<String>,
}

impl Session {
    /// Create a new session with its directory
    pub fn new(portal: &str, options: SessionOptions, builtin_path: &str) -> std::io::Result<Self> {
        let id = SessionId::new();
        let dir = session_dir(&id);

        // Create session directory
        fs::create_dir_all(&dir)?;

        // Write portal type
        fs::write(dir.join("portal"), portal)?;

        // Create bin directory with shims
        let bin_dir = dir.join("bin");
        fs::create_dir_all(&bin_dir)?;

        for cmd in portal_commands(portal) {
            let shim_path = bin_dir.join(cmd);
            let shim_content = format!(
                "#!/bin/sh\nexec \"{}\" \"{}\" \"{}\" \"$@\"\n",
                builtin_path, portal, cmd
            );
            fs::write(&shim_path, shim_content)?;
            fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755))?;
        }

        // Create Unix socket
        let sock_path = dir.join("sock");
        let listener = UnixListener::bind(&sock_path)?;

        Ok(Self {
            id,
            dir,
            child: None,
            listener: Some(listener),
            options,
            selection: Vec::new(),
        })
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
    pub fn run(&mut self) -> std::io::Result<SessionResult> {
        let listener = self.listener.take().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "no listener")
        })?;

        // Set socket to non-blocking for polling
        listener.set_nonblocking(true)?;

        let mut cancelled = false;

        loop {
            // Check if child is still running
            if let Some(ref mut child) = self.child {
                match child.try_wait()? {
                    Some(_status) => {
                        // Child exited normally - confirm with current selection
                        break;
                    }
                    None => {
                        // Still running, check for IPC
                    }
                }
            } else {
                break;
            }

            // Try to accept a connection
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;

                    // Handle request
                    match read_message::<Request>(&mut stream) {
                        Ok(req) => {
                            let resp = self.handle_request(req, &mut cancelled);
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

        // Determine result based on cancelled flag and selection
        let result = if cancelled {
            SessionResult::Cancelled
        } else if self.selection.is_empty() {
            SessionResult::Cancelled
        } else {
            SessionResult::Success {
                uris: self.selection.clone(),
            }
        };

        Ok(result)
    }

    fn handle_request(&mut self, req: Request, cancelled: &mut bool) -> Response {
        match req {
            Request::GetOptions => Response::Options(self.options.clone()),
            Request::GetSelection => Response::Selection(self.selection.clone()),
            Request::Select(uris) => {
                // Just update selection, don't kill terminal
                self.selection = uris;
                Response::Ok
            }
            Request::Cancel => {
                *cancelled = true;
                // Kill the child process
                if let Some(ref mut child) = self.child {
                    let _ = child.kill();
                }
                Response::Ok
            }
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
