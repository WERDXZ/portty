use std::io::BufRead;
use std::process::ExitCode;

use portty_types::ipc::file_chooser::{Request, Response};

use crate::to_uri;

fn send_request(req: &Request) -> Result<Response, portty_types::ipc::IpcError> {
    crate::send_request(req)
}

pub fn dispatch(command: &str, args: &[String]) -> ExitCode {
    match command {
        "select" => cmd_select(args),
        "cancel" => cmd_cancel(),
        _ => {
            eprintln!("Unknown file_chooser command: {command}");
            ExitCode::from(1)
        }
    }
}

fn cmd_select(args: &[String]) -> ExitCode {
    // Parse args manually (--options, --stdin, or files)
    let mut show_options = false;
    let mut from_stdin = false;
    let mut files = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" | "--options" => show_options = true,
            "--stdin" => from_stdin = true,
            "-h" | "--help" => {
                print_select_help();
                return ExitCode::SUCCESS;
            }
            _ if arg.starts_with('-') => {
                eprintln!("Unknown option: {arg}");
                return ExitCode::from(1);
            }
            _ => files.push(arg.clone()),
        }
    }

    if show_options {
        return show_session_options();
    }

    // Get URIs from args or stdin
    let uris: Vec<String> = if from_stdin {
        std::io::stdin()
            .lock()
            .lines()
            .map_while(Result::ok)
            .filter(|l| !l.is_empty())
            .map(|l| to_uri(&l))
            .collect()
    } else {
        files.iter().map(|a| to_uri(a)).collect()
    };

    // If no files provided, show current selection
    if uris.is_empty() {
        return show_selection();
    }

    // Send selection to daemon
    match send_request(&Request::Select(uris)) {
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
            eprintln!("Failed to send selection: {e}");
            ExitCode::from(1)
        }
    }
}

fn print_select_help() {
    println!("Usage: select [OPTIONS] [FILES...]");
    println!();
    println!("Options:");
    println!("  -o, --options  Show session options (filters, title, etc.)");
    println!("  --stdin        Read files from stdin (one per line)");
    println!("  -h, --help     Show this help");
    println!();
    println!("If no files are provided, shows the current selection.");
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
