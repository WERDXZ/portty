use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: portty-builtin <portal> <command> [args...]");
        eprintln!("This binary is meant to be called via shims.");
        return ExitCode::from(1);
    }

    let portal = &args[1];
    let command = &args[2];
    let rest = &args[3..];

    match portal.as_str() {
        "file-chooser" => portty_builtins::file_chooser::dispatch(command, rest),
        _ => {
            eprintln!("Unknown portal: {portal}");
            ExitCode::from(1)
        }
    }
}
