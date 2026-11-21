#![forbid(unsafe_code)]

use clap::Parser;
use clap::error::ErrorKind;
use keel::{Cli, render_json, render_json_error, render_json_info, render_text};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let kind = err.kind();
            let info = matches!(kind, ErrorKind::DisplayHelp | ErrorKind::DisplayVersion);
            let code = if info { 0 } else { 2 };
            let json = args
                .iter()
                .take_while(|a| *a != "--")
                .any(|a| a == "--json");
            if json && info {
                let (command, key) = if kind == ErrorKind::DisplayHelp {
                    ("help", "help")
                } else {
                    ("version", "text")
                };
                println!("{}", render_json_info(command, key, &err.to_string()));
                return ExitCode::from(0);
            }
            let _ = err.print();
            if json && !info {
                let command = command_name(&args);
                let message = err.to_string();
                let fix = format!("run `keel {command} --help`").replace("  ", " ");
                println!(
                    "{}",
                    render_json_error(&command, 2, message.trim_end(), fix.trim_end())
                );
            }
            return ExitCode::from(code);
        }
    };
    let json = cli.json;
    let command = cli.command.name();
    match keel::run(cli) {
        Ok(report) => {
            if json {
                println!("{}", render_json(&report));
            } else {
                print!("{}", render_text(&report));
            }
            ExitCode::from(report.exit_code.clamp(0, 255) as u8)
        }
        Err(err) => {
            eprintln!("error: {err}");
            if json {
                println!(
                    "{}",
                    render_json_error(
                        command,
                        err.exit_code(),
                        &err.to_string(),
                        err.fix()
                            .unwrap_or("the message says what to change; see `keel --help`")
                    )
                );
            }
            ExitCode::from(err.exit_code() as u8)
        }
    }
}

/// The subcommand word of the command line, skipping flags and the value of `-C`.
fn command_name(args: &[String]) -> String {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "-C" {
            it.next();
        } else if a == "--" {
            break;
        } else if !a.starts_with('-') {
            return if ["up", "plan", "doctor"].contains(&a.as_str()) {
                a.clone()
            } else {
                String::new()
            };
        }
    }
    String::new()
}
