use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use portty_ipc::daemon::{DaemonRequest, DaemonResponse, SessionInfo};
use portty_ipc::ipc::file_chooser::{Request as SessionRequest, Response as SessionResponse};
use portty_ipc::ipc::{read_message, write_message};

/// Portty - interact with XDG portal sessions from the command line
#[derive(Parser)]
#[command(name = "portty", version, about)]
struct Cli {
    /// Target a specific session by ID
    #[arg(short, long, global = true)]
    session: Option<String>,

    /// List active sessions
    #[arg(long)]
    list: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Add files to selection
    Select {
        /// Files to select
        files: Vec<String>,

        /// Read files from stdin
        #[arg(long)]
        stdin: bool,
    },

    /// Remove files from selection
    Deselect {
        /// Files to deselect
        files: Vec<String>,
    },

    /// Clear all selection
    Clear,

    /// Submit/confirm the selection
    Submit,

    /// Cancel the operation
    Cancel,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.list {
        return cmd_list();
    }

    match cli.command {
        Some(cmd) => run_command(cli.session, cmd),
        None => {
            // No command - show current selection
            run_command(cli.session, Command::Select { files: vec![], stdin: false })
        }
    }
}

/// Get daemon socket path
fn daemon_socket_path() -> PathBuf {
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata("/proc/self")
        .map(|m| m.uid())
        .unwrap_or(0);
    PathBuf::from(format!("/tmp/portty/{}/daemon.sock", uid))
}

/// Connect to daemon and send request
fn send_daemon_request(req: &DaemonRequest) -> Result<DaemonResponse, String> {
    let sock_path = daemon_socket_path();
    let mut stream = UnixStream::connect(&sock_path)
        .map_err(|e| format!("Failed to connect to daemon: {e}"))?;

    write_message(&mut stream, req)
        .map_err(|e| format!("Failed to send request: {e}"))?;

    read_message(&mut stream)
        .map_err(|e| format!("Failed to read response: {e}"))
}

/// Connect to session socket and send request
fn send_session_request(
    socket_path: &str,
    req: &SessionRequest,
) -> Result<SessionResponse, String> {
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("Failed to connect to session: {e}"))?;

    write_message(&mut stream, req)
        .map_err(|e| format!("Failed to send request: {e}"))?;

    read_message(&mut stream)
        .map_err(|e| format!("Failed to read response: {e}"))
}

/// List active sessions
fn cmd_list() -> ExitCode {
    match send_daemon_request(&DaemonRequest::ListSessions) {
        Ok(DaemonResponse::Sessions(sessions)) => {
            if sessions.is_empty() {
                println!("No active sessions");
            } else {
                for s in sessions {
                    println!(
                        "{} [{}] {}",
                        s.id,
                        s.portal,
                        s.title.as_deref().unwrap_or("")
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Ok(DaemonResponse::Error(e)) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
        Ok(resp) => {
            eprintln!("Unexpected response: {resp:?}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

/// Get session info (auto-select or by ID)
fn get_session(session_id: Option<String>) -> Result<SessionInfo, String> {
    match session_id {
        Some(id) => {
            // Get specific session by ID
            match send_daemon_request(&DaemonRequest::GetSession(id))? {
                DaemonResponse::Session(info) => Ok(info),
                DaemonResponse::Error(e) => Err(e),
                resp => Err(format!("Unexpected response: {resp:?}")),
            }
        }
        None => {
            // Auto-select: list sessions and pick if exactly one
            match send_daemon_request(&DaemonRequest::ListSessions)? {
                DaemonResponse::Sessions(sessions) => {
                    if sessions.is_empty() {
                        Err("No active sessions".to_string())
                    } else if sessions.len() == 1 {
                        Ok(sessions.into_iter().next().unwrap())
                    } else {
                        Err(format!(
                            "Multiple sessions active ({}), specify --session",
                            sessions.len()
                        ))
                    }
                }
                DaemonResponse::Error(e) => Err(e),
                resp => Err(format!("Unexpected response: {resp:?}")),
            }
        }
    }
}

/// Characters that need percent-encoding in file:// URIs (RFC 3986)
const PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// Convert path to file:// URI
fn to_uri(arg: &str) -> Result<String, String> {
    // Already a URI
    if arg.starts_with("file://") || arg.starts_with("http://") || arg.starts_with("https://") {
        return Ok(arg.to_string());
    }

    let path = if arg.starts_with('/') {
        PathBuf::from(arg)
    } else {
        std::env::current_dir()
            .map_err(|e| format!("Failed to get current directory: {e}"))?
            .join(arg)
    };

    let path_str = path.to_string_lossy();
    let encoded = utf8_percent_encode(&path_str, PATH_ENCODE_SET).to_string();
    Ok(format!("file://{}", encoded))
}

/// Run a command on a session
fn run_command(session_id: Option<String>, cmd: Command) -> ExitCode {
    // Get session info from daemon
    let session = match get_session(session_id) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    // Build session request based on command
    let req = match cmd {
        Command::Select { files, stdin } => {
            let uris: Result<Vec<String>, String> = if stdin {
                use std::io::BufRead;
                std::io::stdin()
                    .lock()
                    .lines()
                    .map_while(Result::ok)
                    .filter(|l| !l.is_empty())
                    .map(|l| to_uri(&l))
                    .collect()
            } else {
                files.iter().map(|f| to_uri(f)).collect()
            };

            let uris = match uris {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            if uris.is_empty() {
                // Show current selection
                SessionRequest::GetSelection
            } else {
                SessionRequest::Select(uris)
            }
        }
        Command::Deselect { files } => {
            let uris: Result<Vec<String>, String> = files.iter().map(|f| to_uri(f)).collect();
            let uris = match uris {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };
            SessionRequest::Deselect(uris)
        }
        Command::Clear => SessionRequest::Clear,
        Command::Submit => SessionRequest::Submit,
        Command::Cancel => SessionRequest::Cancel,
    };

    // Send to session socket
    match send_session_request(&session.socket_path, &req) {
        Ok(SessionResponse::Ok) => ExitCode::SUCCESS,
        Ok(SessionResponse::Selection(uris)) => {
            for uri in uris {
                println!("{uri}");
            }
            ExitCode::SUCCESS
        }
        Ok(SessionResponse::Options(opts)) => {
            println!("Title: {}", opts.title);
            println!("Multiple: {}", opts.multiple);
            println!("Directory: {}", opts.directory);
            ExitCode::SUCCESS
        }
        Ok(SessionResponse::Error(e)) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
