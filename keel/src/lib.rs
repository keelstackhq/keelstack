#![forbid(unsafe_code)]

pub mod cli;
pub mod ctx;
pub mod engine;
pub mod error;
pub mod exec;
pub mod manifest;
pub mod pins;
pub mod report;
pub mod step;
pub mod steps;
pub mod tools;

pub use cli::{Cli, Command};
pub use error::KeelError;
pub use report::{render_json, render_json_error, render_json_info, render_text};

use cli::UpArgs;
use ctx::{Ctx, Opts};
use engine::Mode;
use exec::{Cmd, Out, Runner, SystemRunner};
use manifest::Manifest;
use report::Report;
use std::io::{BufRead, IsTerminal};
use std::time::Duration;
use zeroize::Zeroizing;

/// Logs each subprocess call (never its stdin) to stderr.
struct Verbose<'a>(&'a dyn Runner);

impl Runner for Verbose<'_> {
    fn run(&self, cmd: &Cmd) -> Out {
        eprintln!("keel: $ {}", cmd.line());
        self.0.run(cmd)
    }
}

fn read_token() -> std::io::Result<Zeroizing<String>> {
    if std::io::stdin().is_terminal() {
        return rpassword::read_password().map(Zeroizing::new);
    }
    let mut line = Zeroizing::new(String::new());
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

/// Run a parsed command line with the real tools.
pub fn run(cli: Cli) -> Result<Report, KeelError> {
    let sleep = |d: Duration| std::thread::sleep(d);
    run_with(cli, &SystemRunner, &sleep, &read_token)
}

pub fn run_with(
    cli: Cli,
    runner: &dyn Runner,
    sleep: &dyn Fn(Duration),
    stdin: &dyn Fn() -> std::io::Result<Zeroizing<String>>,
) -> Result<Report, KeelError> {
    let verbose = Verbose(runner);
    let runner: &dyn Runner = if cli.verbose { &verbose } else { runner };
    let dir = cli.dir.clone();
    let mut manifest = Manifest::load(&dir)?;
    let (args, command) = match &cli.command {
        Command::Up(a) => (a.clone(), Mode::Up),
        Command::Plan(a) => (a.clone(), Mode::Plan),
        Command::Doctor => (UpArgs::default(), Mode::Plan),
    };
    let doctor = matches!(cli.command, Command::Doctor);
    let changed = manifest.merge_flags(&args)?;
    if manifest.repo.is_none() {
        return Err(KeelError::Fix {
            message: "no repo: pass --repo OWNER/NAME or set `repo` in keel.toml".into(),
            fix: "keel up --repo owner/name --template rust --pieces rust-cli:my-cli".into(),
        });
    }
    if manifest.template.is_none() && !doctor {
        return Err(KeelError::Fix {
            message: "no template: pass --template rust|ts|SOURCE or set `template` in keel.toml"
                .into(),
            fix: "keel up --template rust --pieces rust-cli:my-cli".into(),
        });
    }
    let pins = pins::Pins::load(&dir)?;
    let changed = manifest.default_template_ref(&pins) || changed;
    // `up` writes keel.toml before any other step; `plan` and `doctor` never write.
    if command == Mode::Up && (changed || !Manifest::exists(&dir)) {
        manifest.write(&dir)?;
    }
    let poll_secs = std::env::var("KEEL_POLL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let mut cx = Ctx {
        runner,
        dir,
        manifest,
        pins,
        opts: Opts {
            wait_secs: args.wait,
            poll_secs,
            token_stdin: args.token_stdin,
            dry_run: command == Mode::Plan,
        },
        sleep,
        stdin,
    };
    let mut steps = steps::all_steps();
    if doctor {
        // Preflight (check only) and the doctor check.
        steps.retain(|s| matches!(s.id(), "preflight" | "doctor"));
    }
    let mut report = engine::run_steps(&mut cx, &steps, command)?;
    if doctor {
        report.command = "doctor".into();
    }
    Ok(report)
}
