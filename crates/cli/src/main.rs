use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

use portty_ipc::daemon::{DaemonRequest, DaemonResponse, SessionInfo};
use portty_ipc::ipc::file_chooser::{Request as SessionRequest, Response as SessionResponse};
use portty_ipc::ipc::{read_message, write_message};
use portty_ipc::queue::QueuedCommand;
use portty_ipc::PortalType;

/// Portty - interact with XDG portal sessions from the command line
#[derive(Parser)]
#[command(name = "portty", version, about)]
struct Cli {
    /// Target a specific session by ID
    #[arg(short, long, global = true)]
    session: Option<String>,

    /// Execute immediately instead of queueing
    #[arg(short, long, global = true)]
    immediate: bool,

    /// List active sessions
    #[arg(long)]
    list: bool,

    /// Show queued commands and submissions
    #[arg(long)]
    queue: bool,

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

    /// Bundle pending commands into a submission
    Submit {
        /// Target portal type (default: any)
        #[arg(long)]
        portal: Option<String>,
    },

    /// Cancel - clear pending commands
    Cancel,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.list {
        return cmd_list();
    }

    if cli.queue {
        return cmd_show_queue();
    }

    match cli.command {
        Some(cmd) => {
            if cli.immediate {
                run_immediate(cli.session, cmd)
            } else {
                run_queued(cmd)
            }
        }
        None => {
            // No command - show current selection from session
            run_immediate(cli.session, Command::Select { files: vec![], stdin: false })
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

/// Show queued commands and submissions
fn cmd_show_queue() -> ExitCode {
    match send_daemon_request(&DaemonRequest::QueueStatus) {
        Ok(DaemonResponse::QueueStatus(status)) => {
            if status.pending.is_empty() && status.submissions.is_empty() {
                println!("Queue is empty");
                return ExitCode::SUCCESS;
            }

            if !status.pending.is_empty() {
                println!("Pending commands ({}):", status.pending_count);
                for (i, cmd) in status.pending.iter().enumerate() {
                    print_command(i + 1, cmd, "  ");
                }
            }

            if !status.submissions.is_empty() {
                println!("Submissions ({}):", status.submissions_count);
                for (i, sub) in status.submissions.iter().enumerate() {
                    let portal = sub.portal.map_or("any".to_string(), |p| p.to_string());
                    println!("  {}. [{}] {} command(s)", i + 1, portal, sub.commands.len());
                    for cmd in &sub.commands {
                        print_command(0, cmd, "       ");
                    }
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

fn print_command(num: usize, cmd: &QueuedCommand, indent: &str) {
    match cmd {
        QueuedCommand::Select(uris) => {
            if num > 0 {
                println!("{}{}. select {} file(s)", indent, num, uris.len());
            } else {
                println!("{}select {} file(s)", indent, uris.len());
            }
            for uri in uris {
                println!("{}  {uri}", indent);
            }
        }
        QueuedCommand::Deselect(uris) => {
            if num > 0 {
                println!("{}{}. deselect {} file(s)", indent, num, uris.len());
            } else {
                println!("{}deselect {} file(s)", indent, uris.len());
            }
            for uri in uris {
                println!("{}  {uri}", indent);
            }
        }
        QueuedCommand::Clear => {
            if num > 0 {
                println!("{}{}. clear", indent, num);
            } else {
                println!("{}clear", indent);
            }
        }
    }
}

/// Get session info (auto-select or by ID)
fn get_session(session_id: Option<String>) -> Result<SessionInfo, String> {
    match session_id {
        Some(id) => {
            match send_daemon_request(&DaemonRequest::GetSession(id))? {
                DaemonResponse::Session(info) => Ok(info),
                DaemonResponse::Error(e) => Err(e),
                resp => Err(format!("Unexpected response: {resp:?}")),
            }
        }
        None => {
            match send_daemon_request(&DaemonRequest::ListSessions)? {
                DaemonResponse::Sessions(sessions) => {
                    if sessions.is_empty() {
                        Err("No active sessions".to_string())
                    } else if sessions.len() == 1 {
                        Ok(sessions.into_iter().next().unwrap())
                    } else {
                        eprintln!("Multiple sessions active, choose with --session:");
                        for s in &sessions {
                            eprintln!(
                                "  {} [{}] {}",
                                s.id,
                                s.portal,
                                s.title.as_deref().unwrap_or("")
                            );
                        }
                        Err("Multiple sessions active".to_string())
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

/// Parse files to URIs
fn parse_files(files: &[String], stdin: bool) -> Result<Vec<String>, String> {
    if stdin {
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
    }
}

/// Run command in queued mode (via daemon)
fn run_queued(cmd: Command) -> ExitCode {
    match cmd {
        Command::Select { files, stdin } => {
            let uris = match parse_files(&files, stdin) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            if uris.is_empty() {
                // Show current selection - needs immediate mode
                return run_immediate(None, Command::Select { files: vec![], stdin: false });
            }

            match send_daemon_request(&DaemonRequest::QueuePush(QueuedCommand::Select(uris))) {
                Ok(DaemonResponse::Ok) => {
                    println!("Queued select");
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

        Command::Deselect { files } => {
            let uris = match parse_files(&files, false) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            match send_daemon_request(&DaemonRequest::QueuePush(QueuedCommand::Deselect(uris))) {
                Ok(DaemonResponse::Ok) => {
                    println!("Queued deselect");
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

        Command::Clear => {
            match send_daemon_request(&DaemonRequest::QueuePush(QueuedCommand::Clear)) {
                Ok(DaemonResponse::Ok) => {
                    println!("Queued clear");
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

        Command::Submit { portal } => {
            let portal_type = portal.as_ref().and_then(|s| PortalType::from_str(s));

            // Validate portal string if provided
            if portal.is_some() && portal_type.is_none() {
                eprintln!("Unknown portal type: {}", portal.unwrap());
                return ExitCode::from(1);
            }

            match send_daemon_request(&DaemonRequest::QueueSubmit { portal: portal_type }) {
                Ok(DaemonResponse::Ok) => {
                    let portal_str = portal_type.map_or("any".to_string(), |p| p.to_string());
                    println!("Created submission for [{}]", portal_str);
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

        Command::Cancel => {
            match send_daemon_request(&DaemonRequest::QueueClearPending) {
                Ok(DaemonResponse::Ok) => {
                    println!("Cleared pending commands");
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
    }
}

/// Run command immediately (bypass queue)
fn run_immediate(session_id: Option<String>, cmd: Command) -> ExitCode {
    let session = match get_session(session_id) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let req = match cmd {
        Command::Select { files, stdin } => {
            let uris = match parse_files(&files, stdin) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            if uris.is_empty() {
                SessionRequest::GetSelection
            } else {
                SessionRequest::Select(uris)
            }
        }
        Command::Deselect { files } => {
            let uris = match parse_files(&files, false) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };
            SessionRequest::Deselect(uris)
        }
        Command::Clear => SessionRequest::Clear,
        Command::Submit { .. } => SessionRequest::Submit,
        Command::Cancel => SessionRequest::Cancel,
    };

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
