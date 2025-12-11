use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use libportty::client::{ClientError, DaemonClient};
use libportty::portal::{AddResult, SessionContext};
use libportty::{SessionInfo, files, paths};

/// Portty - interact with XDG portal sessions from the command line
///
/// Auto-detects context:
/// - Inside terminal session (PORTTY_SESSION set): file operations on session dir
/// - Outside: file operations on pending dir, control commands via daemon socket
#[derive(Parser)]
#[command(name = "portty", version, about)]
struct Cli {
    /// Target a specific session by ID (only used outside session)
    #[arg(short, long, global = true)]
    session: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Edit the submission file (add, remove, clear, reset, or print)
    Edit {
        /// Entries to add (raw strings, written as-is)
        #[arg(trailing_var_arg = true)]
        items: Vec<String>,

        /// Read entries from stdin
        #[arg(long)]
        stdin: bool,

        /// Remove entries instead of adding
        #[arg(long)]
        remove: bool,

        /// Clear all entries
        #[arg(long, conflicts_with_all = ["reset", "remove"])]
        clear: bool,

        /// Reset to initial state
        #[arg(long, conflicts_with_all = ["clear", "remove"])]
        reset: bool,
    },

    /// Submit the current submission
    Submit,

    /// Cancel the operation
    Cancel,

    /// Show session info (options + submission)
    Info,

    /// Validate submission against portal constraints
    Verify,

    /// List active sessions
    List,

    /// Show pending and queued submissions
    Queue,
}

/// Execution context
enum Context {
    Session { session_id: String },
    Daemon,
}

fn detect_context() -> Context {
    if let Ok(session_id) = std::env::var("PORTTY_SESSION") {
        Context::Session { session_id }
    } else {
        Context::Daemon
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let ctx = detect_context();

    match cli.command {
        Some(Command::List) => cmd_list(),
        Some(Command::Queue) => cmd_show_queue(),
        Some(cmd) => run_command(ctx, cli.session, cmd),
        None => {
            // No command - show current submission
            run_command(
                ctx,
                cli.session,
                Command::Edit {
                    items: vec![],
                    stdin: false,
                    remove: false,
                    clear: false,
                    reset: false,
                },
            )
        }
    }
}

/// List active sessions
fn cmd_list() -> ExitCode {
    let client = DaemonClient::new();
    match client.list() {
        Ok(sessions) => {
            if sessions.is_empty() {
                println!("No active sessions");
            } else {
                for s in sessions {
                    println!(
                        "{} [{}:{}] {}",
                        s.id,
                        s.portal,
                        s.operation,
                        s.title.as_deref().unwrap_or("")
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

/// Show pending entries and queued submissions
fn cmd_show_queue() -> ExitCode {
    let pending_sub = paths::pending_dir().join("submission");
    let pending_entries = files::read_lines(&pending_sub);

    let subs_dir = paths::base_dir().join("submissions");
    let submissions = read_submissions_dir(&subs_dir);

    if pending_entries.is_empty() && submissions.is_empty() {
        println!("Queue is empty");
        return ExitCode::SUCCESS;
    }

    if !pending_entries.is_empty() {
        println!("Pending entries ({}):", pending_entries.len());
        for entry in &pending_entries {
            println!("  {entry}");
        }
    }

    if !submissions.is_empty() {
        println!("Submissions ({}):", submissions.len());
        for (i, (portal, entries)) in submissions.iter().enumerate() {
            println!("  {}. [{}] {} entry(ies)", i + 1, portal, entries.len());
            for entry in entries {
                println!("       {entry}");
            }
        }
    }

    ExitCode::SUCCESS
}

/// Read submissions directory, return vec of (portal, entries)
fn read_submissions_dir(dir: &Path) -> Vec<(String, Vec<String>)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut dirs: Vec<_> = entries.filter_map(Result::ok).collect();
    dirs.sort_by_key(|e| e.file_name());

    dirs.iter()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let portal = name.split_once('-').map(|(_, p)| p).unwrap_or("unknown");
            let submission = files::read_lines(&path.join("submission"));
            Some((portal.to_string(), submission))
        })
        .collect()
}

/// Get session info from daemon (auto-select or by ID)
fn get_session_info(session_id: Option<String>) -> Result<SessionInfo, ClientError> {
    let client = DaemonClient::new();
    let sessions = client.list()?;

    if let Some(id) = session_id {
        sessions
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| ClientError::Server(format!("Session not found: {id}")))
    } else if sessions.is_empty() {
        Err(ClientError::Server("no active sessions".into()))
    } else if sessions.len() == 1 {
        Ok(sessions.into_iter().next().expect("checked len == 1"))
    } else {
        eprintln!("Multiple sessions active, choose with --session:");
        for s in &sessions {
            eprintln!(
                "  {} [{}:{}] {}",
                s.id,
                s.portal,
                s.operation,
                s.title.as_deref().unwrap_or("")
            );
        }
        Err(ClientError::Server("multiple sessions active".into()))
    }
}

/// Parse items from args or stdin (raw strings, no URI conversion)
fn parse_items(items: &[String], stdin: bool) -> Vec<String> {
    if stdin {
        use std::io::BufRead;
        std::io::stdin()
            .lock()
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.is_empty())
            .collect()
    } else {
        items.to_vec()
    }
}

/// Print lines, showing "(empty)" if none
fn print_lines(lines: &[String]) {
    if lines.is_empty() {
        println!("(empty)");
    } else {
        for line in lines {
            println!("{line}");
        }
    }
}

/// Print session info: raw options.json + submission entries
fn print_session_info(session_dir: &Path) -> ExitCode {
    let options_path = session_dir.join("options.json");
    match fs::read_to_string(&options_path) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("Error reading options: {e}");
            return ExitCode::from(1);
        }
    }

    let sub = session_dir.join("submission");
    let lines = files::read_lines(&sub);
    println!("Submission:");
    if lines.is_empty() {
        println!("  (empty)");
    } else {
        for line in &lines {
            println!("  {line}");
        }
    }

    ExitCode::SUCCESS
}

/// Run command based on context
fn run_command(ctx: Context, session_id: Option<String>, cmd: Command) -> ExitCode {
    match ctx {
        Context::Session { session_id } => run_session_command(&session_id, cmd),
        Context::Daemon => run_daemon_command(session_id, cmd),
    }
}

/// Run command in session context (inside terminal, file ops)
fn run_session_command(session_id: &str, cmd: Command) -> ExitCode {
    let dir = paths::base_dir().join(session_id);
    let sub = dir.join("submission");

    match cmd {
        Command::Edit {
            items,
            stdin,
            remove,
            clear,
            reset,
        } => {
            if clear {
                if let Err(e) = fs::write(&sub, "") {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
                return ExitCode::SUCCESS;
            }

            if reset {
                let client = DaemonClient::new();
                return print_client_result(client.reset(Some(session_id)), "Reset");
            }

            let entries = parse_items(&items, stdin);

            if entries.is_empty() {
                print_lines(&files::read_lines(&sub));
                return ExitCode::SUCCESS;
            }

            if remove {
                if let Err(e) = files::remove_lines(&sub, &entries) {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            } else {
                match SessionContext::from_session_dir(&dir) {
                    Ok(ctx) => match ctx.add_entries(&entries) {
                        Ok(AddResult::Replaced) => {
                            eprintln!("Replaced (single-select mode)");
                        }
                        Ok(AddResult::Appended(_)) => {}
                        Err(e) => {
                            eprintln!("Error: {e}");
                            return ExitCode::from(1);
                        }
                    },
                    Err(e) => {
                        eprintln!("Error detecting session context: {e}");
                        return ExitCode::from(1);
                    }
                }
            }

            ExitCode::SUCCESS
        }

        Command::Info => print_session_info(&dir),

        Command::Verify => {
            let client = DaemonClient::new();
            print_client_result(client.verify(Some(session_id)), "Valid")
        }

        Command::Submit => {
            let client = DaemonClient::new();
            print_client_result(client.submit(Some(session_id)), "Submitted")
        }

        Command::Cancel => {
            let client = DaemonClient::new();
            print_client_result(client.cancel(Some(session_id)), "Cancelled")
        }

        Command::List | Command::Queue => unreachable!(),
    }
}

/// Run command in daemon context (outside terminal)
fn run_daemon_command(session_id: Option<String>, cmd: Command) -> ExitCode {
    let pending = paths::pending_dir();
    let sub = pending.join("submission");

    match cmd {
        Command::Edit {
            items,
            stdin,
            remove,
            clear,
            reset,
        } => {
            if clear {
                let _ = fs::create_dir_all(&pending);
                if let Err(e) = fs::write(&sub, "") {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
                println!("Cleared pending");
                return ExitCode::SUCCESS;
            }

            if reset {
                let client = DaemonClient::new();
                return print_client_result(client.reset(session_id.as_deref()), "Reset");
            }

            let entries = parse_items(&items, stdin);

            if entries.is_empty() {
                // Show submission from active session (if any), or pending
                if let Ok(session) = get_session_info(session_id) {
                    let session_sub = PathBuf::from(&session.dir).join("submission");
                    print_lines(&files::read_lines(&session_sub));
                } else {
                    print_lines(&files::read_lines(&sub));
                }
                return ExitCode::SUCCESS;
            }

            let _ = fs::create_dir_all(&pending);

            if remove {
                if let Err(e) = files::remove_lines(&sub, &entries) {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
                println!("Queued remove");
            } else {
                if let Err(e) = files::append_lines(&sub, &entries) {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
                println!("Queued edit");
            }

            ExitCode::SUCCESS
        }

        Command::Info => {
            let session = match get_session_info(session_id) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            print_session_info(&PathBuf::from(&session.dir))
        }

        Command::Verify => {
            let client = DaemonClient::new();
            print_client_result(client.verify(session_id.as_deref()), "Valid")
        }

        Command::Submit => {
            let client = DaemonClient::new();
            print_client_result(client.submit(session_id.as_deref()), "Submitted")
        }

        Command::Cancel => {
            let client = DaemonClient::new();
            print_client_result(client.cancel(session_id.as_deref()), "Cancelled")
        }

        Command::List | Command::Queue => unreachable!(),
    }
}

/// Print success message or error from a client result
fn print_client_result(result: Result<(), ClientError>, success_msg: &str) -> ExitCode {
    match result {
        Ok(()) => {
            println!("{success_msg}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
    }
}
