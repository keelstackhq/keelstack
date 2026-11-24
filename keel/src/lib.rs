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

fn read_token_blocking() -> std::io::Result<Zeroizing<String>> {
    if std::io::stdin().is_terminal() {
        return rpassword::read_password().map(Zeroizing::new);
    }
    let mut line = Zeroizing::new(String::new());
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

/// The settings of the controlling terminal (`stty -g` on /dev/tty, which rpassword reads) when
/// stdin is a terminal, before the hidden prompt turns echo off; `None` otherwise or when
/// /dev/tty or `stty` is unavailable.
fn tty_settings() -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    // rpassword reads /dev/tty: save the settings of that terminal, not of whatever stdin is.
    let tty = std::fs::File::open("/dev/tty").ok()?;
    let out = std::process::Command::new("stty")
        .arg("-g")
        .stdin(std::process::Stdio::from(tty))
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let saved = String::from_utf8(out.stdout).ok()?.trim().to_string();
    // `stty -g` prints one word of hex fields and colons; anything else is not passed back.
    (!saved.is_empty() && saved.chars().all(|c| c.is_ascii_alphanumeric() || c == ':'))
        .then_some(saved)
}

/// Restore the settings `tty_settings` saved on /dev/tty (echo back on).
fn restore_tty(saved: &str) {
    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return;
    };
    let _ = std::process::Command::new("stty")
        .arg(saved)
        .stdin(std::process::Stdio::from(tty))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Read the token on a helper thread so a signal can stop the wait: the engine then returns
/// `KeelError::Interrupted`, and the token (zeroized on drop) never outlives the read. The
/// hidden prompt's reader thread stays blocked after an interrupt with echo off, so the saved
/// terminal settings are restored then.
fn read_token() -> std::io::Result<Zeroizing<String>> {
    let saved = tty_settings();
    read_token_with(
        read_token_blocking,
        &|| exec::interrupted().is_some(),
        saved.as_deref(),
        &restore_tty,
    )
}

fn read_token_with(
    read: fn() -> std::io::Result<Zeroizing<String>>,
    interrupted: &dyn Fn() -> bool,
    saved_tty: Option<&str>,
    restore: &dyn Fn(&str),
) -> std::io::Result<Zeroizing<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read());
    });
    loop {
        if interrupted() {
            if let Some(s) = saved_tty {
                restore(s);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "interrupted",
            ));
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(r) => return r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(std::io::ErrorKind::BrokenPipe.into());
            }
        }
    }
}

/// Run a parsed command line with the real tools.
pub fn run(cli: Cli) -> Result<Report, KeelError> {
    exec::install_signal_handlers();
    let sleep = |d: Duration| exec::sleep_interruptible(d);
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
    manifest.validate()?;
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
            tool_timeout_secs: tool_timeout_secs(
                std::env::var("KEEL_TOOL_TIMEOUT_SECS").ok().as_deref(),
            ),
            dry_run: command == Mode::Plan,
            cloudflare_token: std::env::var_os("CLOUDFLARE_API_TOKEN")
                .is_some_and(|v| !v.is_empty()),
            cloudflare_account_id: std::env::var("CLOUDFLARE_ACCOUNT_ID")
                .ok()
                .filter(|v| !v.trim().is_empty()),
        },
        verify: None,
        items: Vec::new(),
        pending: Vec::new(),
        sleep,
        stdin,
    };
    let mut steps = steps::steps_for(&cx.manifest);
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

/// `KEEL_TOOL_TIMEOUT_SECS`: a positive number of seconds, else the default (600).
fn tool_timeout_secs(v: Option<&str>) -> u64 {
    v.and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(ctx::DEFAULT_TOOL_TIMEOUT_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_timeout_env_overrides_the_default_of_600_seconds() {
        assert_eq!(tool_timeout_secs(None), 600);
        assert_eq!(tool_timeout_secs(Some("45")), 45);
        for bad in ["", "0", "-3", "soon"] {
            assert_eq!(tool_timeout_secs(Some(bad)), 600, "{bad}");
        }
    }

    fn blocked() -> std::io::Result<Zeroizing<String>> {
        std::thread::sleep(Duration::from_secs(30));
        Ok(Zeroizing::new(String::new()))
    }

    #[test]
    fn an_interrupt_during_the_prompt_restores_the_saved_terminal() {
        let restored = std::cell::RefCell::new(Vec::new());
        let r = read_token_with(blocked, &|| true, Some("500:5:bf:8a3b"), &|s| {
            restored.borrow_mut().push(s.to_string())
        });
        assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(*restored.borrow(), ["500:5:bf:8a3b"]);
        // Not a terminal (nothing saved): nothing to restore.
        let restored = std::cell::RefCell::new(Vec::<String>::new());
        let r = read_token_with(blocked, &|| true, None, &|s| {
            restored.borrow_mut().push(s.to_string())
        });
        assert!(r.is_err());
        assert!(restored.borrow().is_empty());
    }

    #[test]
    fn a_finished_read_restores_nothing() {
        let restored = std::cell::RefCell::new(Vec::<String>::new());
        let r = read_token_with(
            || Ok(Zeroizing::new("tok".into())),
            &|| false,
            Some("x"),
            &|s| restored.borrow_mut().push(s.to_string()),
        );
        assert_eq!(r.unwrap().as_str(), "tok");
        assert!(restored.borrow().is_empty());
    }
}
