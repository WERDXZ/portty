use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

use portty_ipc::ipc::{read_message, write_message};
use portty_ipc::queue::QueuedCommand;
use portty_ipc::{
    DaemonExtension, DaemonRequest, DaemonResponse, DaemonResponseExtension, PortalType, Request,
    Response, SessionInfo, SessionRequest, SessionResponse,
};

/// Portty - interact with XDG portal sessions from the command line
///
/// Auto-detects context:
/// - Inside terminal session (PORTTY_SOCK set): commands go directly to session
/// - Outside: commands are queued, applied when session opens or on submit
#[derive(Parser)]
#[command(name = "portty", version, about)]
struct Cli {
    /// Target a specific session by ID (only used outside session)
    #[arg(short, long, global = true)]
    session: Option<String>,

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
    /// Add files/items to selection
    Select {
        /// Items to select (files for file-chooser)
        items: Vec<String>,

        /// Read items from stdin
        #[arg(long)]
        stdin: bool,
    },

    /// Remove files/items from selection
    Deselect {
        /// Items to deselect
        items: Vec<String>,
    },

    /// Clear all selection
    Clear,

    /// Submit/confirm the selection
    Submit {
        /// Target portal type (only for queue mode, default: any)
        #[arg(long)]
        portal: Option<String>,
    },

    /// Cancel the operation
    Cancel,
}

/// Execution context
enum Context {
    /// Inside a terminal session - talk directly to session socket
    Session { socket_path: String },
    /// Outside - talk to daemon
    Daemon,
}

fn detect_context() -> Context {
    if let Ok(sock) = std::env::var("PORTTY_SOCK") {
        Context::Session { socket_path: sock }
    } else {
        Context::Daemon
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let ctx = detect_context();

    // --list and --queue always go to daemon
    if cli.list {
        return cmd_list();
    }

    if cli.queue {
        return cmd_show_queue();
    }

    match cli.command {
        Some(cmd) => run_command(ctx, cli.session, cmd),
        None => {
            // No command - show current selection
            run_command(ctx, cli.session, Command::Select { items: vec![], stdin: false })
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

/// Send a request to a socket and read response
fn send_request<Req, Resp>(socket_path: &str, req: &Req) -> Result<Resp, String>
where
    Req: portty_ipc::Encode,
    Resp: portty_ipc::Decode<()>,
{
    let mut stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("Failed to connect: {e}"))?;

    write_message(&mut stream, req)
        .map_err(|e| format!("Failed to send request: {e}"))?;

    read_message(&mut stream)
        .map_err(|e| format!("Failed to read response: {e}"))
}

/// Send request to daemon socket
fn send_daemon_request(req: &DaemonRequest) -> Result<DaemonResponse, String> {
    send_request(&daemon_socket_path().to_string_lossy(), req)
}

/// Send request to session socket
fn send_session_request(socket_path: &str, req: &SessionRequest) -> Result<SessionResponse, String> {
    send_request(socket_path, req)
}

/// List active sessions
fn cmd_list() -> ExitCode {
    match send_daemon_request(&Request::Extended(DaemonExtension::ListSessions)) {
        Ok(Response::Extended(DaemonResponseExtension::Sessions(sessions))) => {
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
        Ok(Response::Error(e)) => {
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
    match send_daemon_request(&Request::Extended(DaemonExtension::QueueStatus)) {
        Ok(Response::Extended(DaemonResponseExtension::QueueStatus(status))) => {
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
        Ok(Response::Error(e)) => {
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
                println!("{}{}. select {} item(s)", indent, num, uris.len());
            } else {
                println!("{}select {} item(s)", indent, uris.len());
            }
            for uri in uris {
                println!("{}  {uri}", indent);
            }
        }
        QueuedCommand::Deselect(uris) => {
            if num > 0 {
                println!("{}{}. deselect {} item(s)", indent, num, uris.len());
            } else {
                println!("{}deselect {} item(s)", indent, uris.len());
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
            match send_daemon_request(&Request::Extended(DaemonExtension::GetSession(id)))? {
                Response::Extended(DaemonResponseExtension::Session(info)) => Ok(info),
                Response::Error(e) => Err(e),
                resp => Err(format!("Unexpected response: {resp:?}")),
            }
        }
        None => {
            match send_daemon_request(&Request::Extended(DaemonExtension::ListSessions))? {
                Response::Extended(DaemonResponseExtension::Sessions(sessions)) => {
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
                Response::Error(e) => Err(e),
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

/// Parse items to URIs
fn parse_items(items: &[String], stdin: bool) -> Result<Vec<String>, String> {
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
        items.iter().map(|f| to_uri(f)).collect()
    }
}

/// Run command based on context
fn run_command(ctx: Context, session_id: Option<String>, cmd: Command) -> ExitCode {
    match ctx {
        Context::Session { socket_path } => run_session_command(&socket_path, cmd),
        Context::Daemon => run_daemon_command(session_id, cmd),
    }
}

/// Run command directly on session socket (inside terminal)
fn run_session_command(socket_path: &str, cmd: Command) -> ExitCode {
    let req: SessionRequest = match cmd {
        Command::Select { items, stdin } => {
            let uris = match parse_items(&items, stdin) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            if uris.is_empty() {
                Request::GetSelection
            } else {
                Request::Select(uris)
            }
        }
        Command::Deselect { items } => {
            let uris = match parse_items(&items, false) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };
            Request::Deselect(uris)
        }
        Command::Clear => Request::Clear,
        Command::Submit { .. } => Request::Submit,
        Command::Cancel => Request::Cancel,
    };

    match send_session_request(socket_path, &req) {
        Ok(Response::Ok) => ExitCode::SUCCESS,
        Ok(Response::Selection(uris)) => {
            for uri in uris {
                println!("{uri}");
            }
            ExitCode::SUCCESS
        }
        Ok(Response::Options(opts)) => {
            println!("Title: {}", opts.title);
            println!("Multiple: {}", opts.multiple);
            println!("Directory: {}", opts.directory);
            ExitCode::SUCCESS
        }
        Ok(Response::Error(e)) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
        Ok(Response::Extended(never)) => match never {},
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

/// Run command via daemon (queue mode, outside terminal)
fn run_daemon_command(session_id: Option<String>, cmd: Command) -> ExitCode {
    match cmd {
        Command::Select { items, stdin } => {
            let uris = match parse_items(&items, stdin) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            if uris.is_empty() {
                // Show current selection - need active session
                let session = match get_session(session_id) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        return ExitCode::from(1);
                    }
                };
                return run_session_command(&session.socket_path, Command::Select { items: vec![], stdin: false });
            }

            match send_daemon_request(&Request::Extended(DaemonExtension::QueuePush(QueuedCommand::Select(uris)))) {
                Ok(Response::Ok) => {
                    println!("Queued select");
                    ExitCode::SUCCESS
                }
                Ok(Response::Error(e)) => {
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

        Command::Deselect { items } => {
            let uris = match parse_items(&items, false) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            match send_daemon_request(&Request::Extended(DaemonExtension::QueuePush(QueuedCommand::Deselect(uris)))) {
                Ok(Response::Ok) => {
                    println!("Queued deselect");
                    ExitCode::SUCCESS
                }
                Ok(Response::Error(e)) => {
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
            match send_daemon_request(&Request::Extended(DaemonExtension::QueuePush(QueuedCommand::Clear))) {
                Ok(Response::Ok) => {
                    println!("Queued clear");
                    ExitCode::SUCCESS
                }
                Ok(Response::Error(e)) => {
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
            let portal_type = portal.as_ref().map(|s| s.parse::<PortalType>());

            if let Some(Err(ref e)) = portal_type {
                eprintln!("{e}");
                return ExitCode::from(1);
            }

            let portal_type = portal_type.and_then(|r| r.ok());

            match send_daemon_request(&Request::Extended(DaemonExtension::QueueSubmit { portal: portal_type })) {
                Ok(Response::Ok) => {
                    let portal_str = portal_type.map_or("any".to_string(), |p| p.to_string());
                    println!("Submitted for [{}]", portal_str);
                    ExitCode::SUCCESS
                }
                Ok(Response::Error(e)) => {
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
            match send_daemon_request(&Request::Extended(DaemonExtension::QueueClearPending)) {
                Ok(Response::Ok) => {
                    println!("Cleared pending commands");
                    ExitCode::SUCCESS
                }
                Ok(Response::Error(e)) => {
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
