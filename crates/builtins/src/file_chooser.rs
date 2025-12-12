use std::io::BufRead;
use std::process::ExitCode;

use clap::Parser;
use portty_types::ipc::file_chooser::{Request, Response};

use crate::to_uri;

fn send_request(req: &Request) -> Result<Response, portty_types::ipc::IpcError> {
    crate::send_request(req)
}

pub fn dispatch(command: &str, args: &[String]) -> ExitCode {
    match command {
        "select" => cmd_select(args),
        "submit" => cmd_submit(),
        "cancel" => cmd_cancel(),
        _ => {
            eprintln!("Unknown file_chooser command: {command}");
            ExitCode::from(1)
        }
    }
}

#[derive(Parser)]
#[command(name = "select", about = "Manage file selection")]
struct SelectArgs {
    /// Show session options (filters, title, etc.)
    #[arg(short = 'o', long)]
    options: bool,

    /// Clear all selection
    #[arg(short, long)]
    clear: bool,

    /// Remove files from selection (instead of adding)
    #[arg(short, long)]
    deselect: bool,

    /// Read files from stdin (one per line)
    #[arg(long)]
    stdin: bool,

    /// Files to select
    files: Vec<String>,
}

fn cmd_select(args: &[String]) -> ExitCode {
    let args = match SelectArgs::try_parse_from(
        std::iter::once("select".to_string()).chain(args.iter().cloned()),
    ) {
        Ok(args) => args,
        Err(e) => {
            e.print().ok();
            return if e.kind() == clap::error::ErrorKind::DisplayHelp
                || e.kind() == clap::error::ErrorKind::DisplayVersion
            {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            };
        }
    };

    if args.options {
        return show_session_options();
    }

    if args.clear {
        return clear_selection();
    }

    // Get URIs from args or stdin
    let uris: Vec<String> = if args.stdin {
        std::io::stdin()
            .lock()
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.is_empty())
            .map(|l| to_uri(&l))
            .collect()
    } else {
        args.files.iter().map(|a| to_uri(a)).collect()
    };

    // If no files provided, show current selection
    if uris.is_empty() {
        return show_selection();
    }

    // Send request to daemon
    let request = if args.deselect {
        Request::Deselect(uris)
    } else {
        Request::Select(uris)
    };

    match send_request(&request) {
        Ok(Response::Ok) => ExitCode::SUCCESS,
        Ok(Response::Error(e)) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
        Ok(resp) => {
            eprintln!("Unexpected response: {resp:?}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("Failed to send request: {e}");
            ExitCode::from(1)
        }
    }
}

fn clear_selection() -> ExitCode {
    match send_request(&Request::Clear) {
        Ok(Response::Ok) => ExitCode::SUCCESS,
        Ok(Response::Error(e)) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
        Ok(resp) => {
            eprintln!("Unexpected response: {resp:?}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("Failed to clear selection: {e}");
            ExitCode::from(1)
        }
    }
}

fn show_session_options() -> ExitCode {
    match send_request(&Request::GetOptions) {
        Ok(Response::Options(opts)) => {
            println!("Title: {}", opts.title);
            println!("Multiple: {}", opts.multiple);
            println!("Directory: {}", opts.directory);
            println!("Save mode: {}", opts.save_mode);
            if let Some(name) = &opts.current_name {
                println!("Current name: {name}");
            }
            if let Some(folder) = &opts.current_folder {
                println!("Current folder: {folder}");
            }
            if !opts.filters.is_empty() {
                println!("Filters:");
                for filter in &opts.filters {
                    println!("  {}: {:?}", filter.name, filter.patterns);
                }
            }
            ExitCode::SUCCESS
        }
        Ok(resp) => {
            eprintln!("Unexpected response: {resp:?}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("Failed to get options: {e}");
            ExitCode::from(1)
        }
    }
}

fn show_selection() -> ExitCode {
    match send_request(&Request::GetSelection) {
        Ok(Response::Selection(uris)) => {
            for uri in &uris {
                println!("{uri}");
            }
            if uris.is_empty() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Ok(resp) => {
            eprintln!("Unexpected response: {resp:?}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("Failed to get selection: {e}");
            ExitCode::from(1)
        }
    }
}


fn cmd_submit() -> ExitCode {
    match send_request(&Request::Submit) {
        Ok(Response::Ok) => ExitCode::SUCCESS,
        Ok(Response::Error(e)) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
        Ok(resp) => {
            eprintln!("Unexpected response: {resp:?}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("Failed to submit: {e}");
            ExitCode::from(1)
        }
    }
}

fn cmd_cancel() -> ExitCode {
    match send_request(&Request::Cancel) {
        Ok(Response::Ok) => ExitCode::SUCCESS,
        Ok(Response::Error(e)) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
        Ok(resp) => {
            eprintln!("Unexpected response: {resp:?}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("Failed to cancel: {e}");
            ExitCode::from(1)
        }
    }
}
