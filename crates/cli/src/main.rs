use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use libportty::client::{ClientError, DaemonClient};
use libportty::portal::intent::queue;
use libportty::portal::{AddResult, Intent, MergeOp, SessionContext, parse_item};
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
    /// Add typed items to the current queue or session
    Add {
        /// Item family: path, directory, or color
        family: String,

        /// Items to add
        #[arg(trailing_var_arg = true)]
        items: Vec<String>,

        /// Read items from stdin
        #[arg(long)]
        stdin: bool,
    },

    /// Replace the current queue or session with typed items
    Set {
        /// Item family: path, directory, or color
        family: String,

        /// Items to set
        #[arg(trailing_var_arg = true)]
        items: Vec<String>,

        /// Read items from stdin
        #[arg(long)]
        stdin: bool,
    },

    /// Remove typed items from the current queue or session
    Remove {
        /// Item family: path, directory, or color
        family: String,

        /// Items to remove
        #[arg(trailing_var_arg = true)]
        items: Vec<String>,

        /// Read items from stdin
        #[arg(long)]
        stdin: bool,
    },

    /// Clear the current queue or session submission
    Clear,

    /// Reset a live session submission to its initial state
    Reset,

    /// Show the current queue or session submission
    Show,

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
        None => run_command(ctx, cli.session, Command::Show),
    }
}

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

fn cmd_show_queue() -> ExitCode {
    let pending_dir = paths::pending_dir();
    let pending_intent = queue::read(&pending_dir);

    let subs_dir = paths::base_dir().join("submissions");
    let submissions = read_submissions_dir(&subs_dir);

    if pending_intent.is_none() && submissions.is_empty() {
        println!("Queue is empty");
        return ExitCode::SUCCESS;
    }

    if let Some(intent) = pending_intent {
        println!("Pending intent:");
        print!("{intent}");
    }

    if !submissions.is_empty() {
        println!("Submissions ({}):", submissions.len());
        for (i, (portal, intent)) in submissions.iter().enumerate() {
            println!("  {}. [{}]", i + 1, portal);
            print!("{intent}");
            if i + 1 != submissions.len() {
                println!();
            }
        }
    }

    ExitCode::SUCCESS
}

fn read_submissions_dir(dir: &Path) -> Vec<(String, Intent)> {
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
            let intent = queue::read(&path)?;
            Some((portal.to_string(), intent))
        })
        .collect()
}

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

fn parse_intent(family: &str, items: &[String], stdin: bool) -> Result<Intent, String> {
    let values = parse_items(items, stdin);
    if values.is_empty() {
        return Err("no items provided".to_string());
    }

    let parsed: Result<Vec<_>, _> = values
        .iter()
        .map(|value| parse_item(family, value))
        .collect();
    let parsed = parsed?;

    let mut intent = Intent::default();
    intent.apply(&parsed, MergeOp::Set)?;
    Ok(intent)
}

fn print_lines(lines: &[String]) {
    if lines.is_empty() {
        println!("(empty)");
    } else {
        for line in lines {
            println!("{line}");
        }
    }
}

fn print_intent(intent: &Intent) {
    print!("{intent}");
}

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

fn run_command(ctx: Context, session_id: Option<String>, cmd: Command) -> ExitCode {
    match ctx {
        Context::Session { session_id } => run_session_command(&session_id, cmd),
        Context::Daemon => run_daemon_command(session_id, cmd),
    }
}

fn handle_add_result(result: AddResult) {
    match result {
        AddResult::Replaced => eprintln!("Replaced (single-select mode)"),
        AddResult::Appended(_) => {}
    }
}

fn parse_intent_items(
    family: &str,
    items: &[String],
    stdin: bool,
) -> Result<Vec<libportty::portal::IntentItem>, String> {
    let values = parse_items(items, stdin);
    if values.is_empty() {
        return Err("no items provided".to_string());
    }

    values
        .iter()
        .map(|value| parse_item(family, value))
        .collect()
}

fn resolve_target_session_dir(session_id: Option<String>) -> Result<Option<PathBuf>, ClientError> {
    match session_id {
        Some(session_id) => {
            get_session_info(Some(session_id)).map(|session| Some(PathBuf::from(session.dir)))
        }
        None => Ok(None),
    }
}

fn resolve_live_session_dir(session_id: Option<String>) -> Result<PathBuf, ClientError> {
    get_session_info(session_id).map(|session| PathBuf::from(session.dir))
}

fn run_session_command(session_id: &str, cmd: Command) -> ExitCode {
    let dir = paths::base_dir().join(session_id);
    let sub = dir.join("submission");

    match cmd {
        Command::Add {
            family,
            items,
            stdin,
        } => {
            let intent = match parse_intent(&family, &items, stdin) {
                Ok(intent) => intent,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            match SessionContext::from_session_dir(&dir) {
                Ok(ctx) => match ctx.add_intent(&intent) {
                    Ok(result) => handle_add_result(result),
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

            ExitCode::SUCCESS
        }
        Command::Set {
            family,
            items,
            stdin,
        } => {
            let intent = match parse_intent(&family, &items, stdin) {
                Ok(intent) => intent,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            match SessionContext::from_session_dir(&dir) {
                Ok(ctx) => {
                    if let Err(e) = ctx.set_intent(&intent) {
                        eprintln!("Error: {e}");
                        return ExitCode::from(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error detecting session context: {e}");
                    return ExitCode::from(1);
                }
            }

            ExitCode::SUCCESS
        }
        Command::Remove {
            family,
            items,
            stdin,
        } => {
            let intent = match parse_intent(&family, &items, stdin) {
                Ok(intent) => intent,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            match SessionContext::from_session_dir(&dir) {
                Ok(ctx) => {
                    if let Err(e) = ctx.remove_intent(&intent) {
                        eprintln!("Error: {e}");
                        return ExitCode::from(1);
                    }
                }
                Err(e) => {
                    eprintln!("Error detecting session context: {e}");
                    return ExitCode::from(1);
                }
            }

            ExitCode::SUCCESS
        }
        Command::Clear => {
            if let Err(e) = fs::write(&sub, "") {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Command::Reset => {
            let client = DaemonClient::new();
            print_client_result(client.reset(Some(session_id)), "Reset")
        }
        Command::Show => {
            print_lines(&files::read_lines(&sub));
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

fn run_daemon_command(session_id: Option<String>, cmd: Command) -> ExitCode {
    let pending = paths::pending_dir();

    match cmd {
        Command::Add {
            family,
            items,
            stdin,
        } => {
            let intent = match parse_intent(&family, &items, stdin) {
                Ok(intent) => intent,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            let target_dir = match resolve_target_session_dir(session_id) {
                Ok(dir) => dir,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            if let Some(dir) = target_dir {
                let ctx = match SessionContext::from_session_dir(&dir) {
                    Ok(ctx) => ctx,
                    Err(e) => {
                        eprintln!("Error detecting session context: {e}");
                        return ExitCode::from(1);
                    }
                };

                match ctx.add_intent(&intent) {
                    Ok(result) => handle_add_result(result),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        return ExitCode::from(1);
                    }
                }
                return ExitCode::SUCCESS;
            }

            let mut existing = queue::read(&pending).unwrap_or_default();
            if let Err(e) = existing.apply(&intent.items, MergeOp::Add) {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
            if let Err(e) = queue::write(&pending, &existing) {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
            println!("Queued {} item(s)", intent.items.len());
            ExitCode::SUCCESS
        }
        Command::Set {
            family,
            items,
            stdin,
        } => {
            let intent = match parse_intent(&family, &items, stdin) {
                Ok(intent) => intent,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            let target_dir = match resolve_target_session_dir(session_id) {
                Ok(dir) => dir,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            if let Some(dir) = target_dir {
                let ctx = match SessionContext::from_session_dir(&dir) {
                    Ok(ctx) => ctx,
                    Err(e) => {
                        eprintln!("Error detecting session context: {e}");
                        return ExitCode::from(1);
                    }
                };

                if let Err(e) = ctx.set_intent(&intent) {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
                return ExitCode::SUCCESS;
            }

            if let Err(e) = queue::write(&pending, &intent) {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
            println!("Queued replacement");
            ExitCode::SUCCESS
        }
        Command::Remove {
            family,
            items,
            stdin,
        } => {
            let target_dir = match resolve_target_session_dir(session_id.clone()) {
                Ok(dir) => dir,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            if let Some(dir) = target_dir {
                let intent = match parse_intent(&family, &items, stdin) {
                    Ok(intent) => intent,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        return ExitCode::from(1);
                    }
                };
                let ctx = match SessionContext::from_session_dir(&dir) {
                    Ok(ctx) => ctx,
                    Err(e) => {
                        eprintln!("Error detecting session context: {e}");
                        return ExitCode::from(1);
                    }
                };

                if let Err(e) = ctx.remove_intent(&intent) {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
                return ExitCode::SUCCESS;
            }

            let items = match parse_intent_items(&family, &items, stdin) {
                Ok(items) => items,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            let mut existing = match queue::read(&pending) {
                Some(intent) => intent,
                None => {
                    eprintln!("Error: no pending intent to remove from");
                    return ExitCode::from(1);
                }
            };

            if let Err(e) = existing.remove(&items) {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }

            if existing.is_empty() {
                if let Err(e) = queue::clear(&pending) {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            } else if let Err(e) = queue::write(&pending, &existing) {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }

            ExitCode::SUCCESS
        }
        Command::Clear => {
            let target_dir = match resolve_target_session_dir(session_id.clone()) {
                Ok(dir) => dir,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };

            if let Some(dir) = target_dir {
                if let Err(e) = fs::write(dir.join("submission"), "") {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            } else if let Err(e) = queue::clear(&pending) {
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        Command::Reset => {
            let client = DaemonClient::new();
            print_client_result(client.reset(session_id.as_deref()), "Reset")
        }
        Command::Show => {
            match resolve_live_session_dir(session_id.clone()) {
                Ok(dir) => print_lines(&files::read_lines(&dir.join("submission"))),
                Err(ClientError::Server(msg)) if msg == "no active sessions" => {
                    if let Some(intent) = queue::read(&pending) {
                        print_intent(&intent);
                    } else {
                        println!("(empty)");
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
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
