#![forbid(unsafe_code)]

pub mod cli;
pub mod ctx;
pub mod engine;
pub mod error;
pub mod exec;
pub mod manifest;
pub mod migrate;
pub mod pins;
pub mod report;
pub mod step;
pub mod steps;
pub mod tools;

pub use cli::{Cli, Command};
pub use error::KeelError;
pub use migrate::{render_migrate_json, render_migrate_text};
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
    valid_tty_settings(&saved).then_some(saved)
}

/// `stty -g` prints one word: hex fields and colons (Linux), or `name=value` fields such as
/// `gfmt1:cflag=4b00:iflag=2b02` (macOS). Anything else is not passed back to `stty`.
fn valid_tty_settings(saved: &str) -> bool {
    !saved.is_empty()
        && saved
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == '=')
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
        || exec::interrupted().is_some(),
        saved,
        std::sync::Arc::new(restore_tty),
    )
}

type Restore = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

fn read_token_with(
    read: fn() -> std::io::Result<Zeroizing<String>>,
    interrupted: fn() -> bool,
    saved_tty: Option<String>,
    restore: Restore,
) -> std::io::Result<Zeroizing<String>> {
    let stop = |saved: &Option<String>, restore: &Restore| {
        if let Some(s) = saved {
            restore(s);
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "interrupted",
        ))
    };
    // Interrupted already: no prompt (and no reader thread) at all.
    if interrupted() {
        return stop(&saved_tty, &restore);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let (saved, again) = (saved_tty.clone(), restore.clone());
    std::thread::spawn(move || {
        let r = read();
        // The prompt outlived an interrupt (keel stopped waiting for it): when it finally ends,
        // its own cleanup may have run on stale settings, so restore the saved ones again.
        if interrupted()
            && let Some(s) = &saved
        {
            again(s);
        }
        let _ = tx.send(r);
    });
    loop {
        if interrupted() {
            return stop(&saved_tty, &restore);
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

/// Run `keel migrate` with the real tools.
pub fn run_migrate(cli: &Cli) -> Result<migrate::MigrateReport, KeelError> {
    exec::install_signal_handlers();
    let Command::Migrate(args) = &cli.command else {
        return Err(KeelError::Usage("not a migrate command".into()));
    };
    let verbose = Verbose(&SystemRunner);
    let runner: &dyn Runner = if cli.verbose { &verbose } else { &SystemRunner };
    migrate::run(&cli.dir, args, runner)
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
        Command::Migrate(_) => {
            return Err(KeelError::Usage(
                "`keel migrate` has its own report: call keel::run_migrate".into(),
            ));
        }
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

    #[test]
    fn stty_g_output_of_linux_and_macos_is_accepted() {
        assert!(valid_tty_settings("4500:5:bf:8a3b:3:1c:7f:15:4:0:1:0"));
        assert!(valid_tty_settings(
            "gfmt1:cflag=4b00:iflag=6b02:lflag=200005cb:oflag=3:discard=f:eof=4:eol=ff"
        ));
        for bad in ["", "a b", "a;b", "$(x)", "a\nb", "-a", "a=b c"] {
            assert!(!valid_tty_settings(bad), "{bad:?}");
        }
    }

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn recorder() -> (Arc<Mutex<Vec<String>>>, Restore) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        (
            seen,
            Arc::new(move |x: &str| s.lock().unwrap().push(x.to_string())),
        )
    }

    static EARLY_READ: AtomicBool = AtomicBool::new(false);
    fn early_read() -> std::io::Result<Zeroizing<String>> {
        EARLY_READ.store(true, Ordering::SeqCst);
        Ok(Zeroizing::new(String::new()))
    }

    #[test]
    fn interrupted_before_the_prompt_reads_nothing_and_restores() {
        let (seen, restore) = recorder();
        let r = read_token_with(early_read, || true, Some("500:5:bf".into()), restore);
        assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        std::thread::sleep(Duration::from_millis(100));
        assert!(!EARLY_READ.load(Ordering::SeqCst), "no reader started");
        assert_eq!(*seen.lock().unwrap(), ["500:5:bf"]);
        // Not a terminal (nothing saved): nothing to restore.
        let (seen, restore) = recorder();
        assert!(read_token_with(early_read, || true, None, restore).is_err());
        assert!(seen.lock().unwrap().is_empty());
    }

    static MID: AtomicUsize = AtomicUsize::new(0);
    fn mid_interrupted() -> bool {
        MID.load(Ordering::SeqCst) > 0
    }
    fn mid_read() -> std::io::Result<Zeroizing<String>> {
        MID.store(1, Ordering::SeqCst); // the signal arrives while the prompt is up
        std::thread::sleep(Duration::from_millis(300));
        Ok(Zeroizing::new(String::new()))
    }

    #[test]
    fn an_interrupt_during_the_prompt_restores_now_and_again_when_the_reader_ends() {
        let (seen, restore) = recorder();
        let r = read_token_with(mid_read, mid_interrupted, Some("500:5:bf".into()), restore);
        assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(
            *seen.lock().unwrap(),
            ["500:5:bf"],
            "restored at the interrupt"
        );
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(
            *seen.lock().unwrap(),
            ["500:5:bf", "500:5:bf"],
            "restored again once the reader stopped"
        );
    }

    #[test]
    fn a_finished_read_restores_nothing() {
        let (seen, restore) = recorder();
        let r = read_token_with(
            || Ok(Zeroizing::new("tok".into())),
            || false,
            Some("x".into()),
            restore,
        );
        assert_eq!(r.unwrap().as_str(), "tok");
        assert!(seen.lock().unwrap().is_empty());
    }
}
