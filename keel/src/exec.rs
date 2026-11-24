use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once, mpsc};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// One subprocess call. `stdin` carries secrets: it is never printed (see the `Debug` impl).
#[derive(Clone)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub dir: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<Zeroizing<Vec<u8>>>,
    /// The real runner kills the child after this long.
    pub timeout: Option<Duration>,
}

impl Cmd {
    pub fn new(program: &str, args: &[&str]) -> Cmd {
        Cmd {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            dir: None,
            env: Vec::new(),
            stdin: None,
            timeout: None,
        }
    }

    /// Kill the child and report "timed out after N s" when it runs longer than `t`.
    pub fn timeout(mut self, t: Duration) -> Cmd {
        self.timeout = Some(t);
        self
    }

    pub fn dir(mut self, dir: impl Into<PathBuf>) -> Cmd {
        self.dir = Some(dir.into());
        self
    }

    pub fn stdin(mut self, data: Vec<u8>) -> Cmd {
        self.stdin = Some(Zeroizing::new(data));
        self
    }

    /// `program arg arg`, for logs and match keys; never includes stdin.
    pub fn line(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl std::fmt::Debug for Cmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cmd")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("dir", &self.dir)
            .field("stdin", &self.stdin.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// `code` -1 means the program was not found (or could not start).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Out {
    pub fn ok(stdout: &str) -> Out {
        Out::with(0, stdout, "")
    }

    pub fn with(code: i32, stdout: &str, stderr: &str) -> Out {
        Out {
            code,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }
}

pub trait Runner {
    fn run(&self, cmd: &Cmd) -> Out;
}

/// Send `sig` (a `kill` name such as `KILL`) to the process group `pid` leads (`group`), or to
/// the child alone. Safe code only: the crate forbids `unsafe`, so this shells out to `kill`.
#[cfg(unix)]
fn signal_child(sig: &str, pid: u32, group: bool) {
    let target = if group {
        format!("-{pid}")
    } else {
        pid.to_string()
    };
    let _ = Command::new("kill")
        .args([&format!("-{sig}"), "--", &target])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// The last SIGINT/SIGTERM number keel received (0: none). Set by handlers registered once per
/// process and never unregistered, so Ctrl-C never falls back to being ignored.
#[cfg(unix)]
fn got_sig() -> &'static std::sync::Arc<AtomicUsize> {
    static SIG: std::sync::OnceLock<std::sync::Arc<AtomicUsize>> = std::sync::OnceLock::new();
    SIG.get_or_init(|| std::sync::Arc::new(AtomicUsize::new(0)))
}

/// Register the SIGINT/SIGTERM handlers (once per process). They only record the signal.
pub fn install_signal_handlers() {
    static HANDLERS: Once = Once::new();
    HANDLERS.call_once(|| {
        #[cfg(unix)]
        for s in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            let _ = signal_hook::flag::register_usize(s, got_sig().clone(), s as usize);
        }
    });
}

/// The signal number keel was interrupted by, if any (its exit code is 128 + that number).
pub fn interrupted() -> Option<i32> {
    #[cfg(unix)]
    {
        let s = got_sig().load(Ordering::SeqCst);
        if s != 0 {
            return Some(s as i32);
        }
    }
    None
}

/// Sleep `d` in short slices; returns early once keel is interrupted.
pub fn sleep_interruptible(d: Duration) {
    let end = Instant::now() + d;
    while interrupted().is_none() {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_millis(50)));
    }
}

/// How long keel reads a finished child's pipes after a normal exit: what the child wrote is
/// already buffered, so only a grandchild that keeps the pipes open can make it wait this long.
pub const PIPE_GRACE: Duration = Duration::from_secs(10);

#[cfg(test)]
thread_local! {
    /// Tests only: a shorter grace after a normal exit.
    static TEST_GRACE: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

/// The one deadline (from now) for reading both pipes once the child is gone.
fn pipe_grace(timed_out: bool, forwarded: bool) -> Duration {
    if timed_out {
        Duration::from_secs(1)
    } else if forwarded {
        Duration::from_secs(2)
    } else {
        #[cfg(test)]
        if let Some(g) = TEST_GRACE.get() {
            return g;
        }
        PIPE_GRACE
    }
}

/// Whether an interrupt ends the after-exit pipe wait at once: yes after a normal exit (Ctrl-C
/// must not hold keel for `PIPE_GRACE`), no once a signal was forwarded (the interrupt is why the
/// child stopped: its trailing output still gets the 2 s grace).
fn interruptible_wait(forwarded: bool) -> bool {
    !forwarded
}

/// Wait for a pipe reader's EOF signal until `until`, in short slices, and stop at once when
/// `interrupted` says so (Ctrl-C during the after-exit wait must not hold keel).
fn wait_eof(rx: &mpsc::Receiver<()>, until: Instant, interrupted: &dyn Fn() -> bool) -> bool {
    loop {
        if interrupted() {
            return false;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        match rx.recv_timeout(left.min(Duration::from_millis(50))) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Ok(()) => return true,
            Err(mpsc::RecvTimeoutError::Disconnected) => return false,
        }
    }
}

/// Runs real subprocesses. Stdin is piped only when the call has some; it is never logged.
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, cmd: &Cmd) -> Out {
        let mut c = Command::new(&cmd.program);
        c.args(&cmd.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(if cmd.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        if let Some(d) = &cmd.dir {
            c.current_dir(d);
        }
        for (k, v) in &cmd.env {
            c.env(k, v);
        }
        // Own process group only when a timeout must stop the grandchildren too (flyctl under
        // fly-setup.sh). Runs without a timeout stay in keel's group, so the child is never a
        // background group (no SIGTTIN on tty reads) and Ctrl-C reaches it directly.
        #[cfg(unix)]
        let grouped = cmd.timeout.is_some();
        #[cfg(unix)]
        if grouped {
            c.process_group(0);
        }
        // SIGINT/SIGTERM sent to keel are forwarded to the child, so it is never orphaned. Once
        // interrupted, no new child starts: the engine turns the flag into an error.
        install_signal_handlers();
        if interrupted().is_some() {
            return Out::with(-1, "", "interrupted");
        }
        let mut child = match c.spawn() {
            Ok(ch) => ch,
            Err(e) => return Out::with(-1, "", &e.to_string()),
        };
        if let Some(data) = &cmd.stdin
            && let Some(mut pipe) = child.stdin.take()
        {
            // A child that exits early closes the pipe: not an error here.
            let _ = pipe.write_all(data);
        }
        // Both pipes are drained on threads so a chatty child never blocks on a full pipe. Each
        // thread appends what it reads to a shared buffer and says when it reached EOF, so what
        // the child wrote is kept even when a grandchild holds the pipe past the grace.
        let reader = |pipe: Option<Box<dyn Read + Send>>| {
            let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let (tx, rx) = mpsc::channel::<()>();
            let sink = buf.clone();
            std::thread::spawn(move || {
                if let Some(mut p) = pipe {
                    let mut chunk = [0u8; 8192];
                    loop {
                        match p.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(n) => sink
                                .lock()
                                .map_or((), |mut b| b.extend_from_slice(&chunk[..n])),
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(_) => break,
                        }
                    }
                }
                let _ = tx.send(());
            });
            (buf, rx)
        };
        let out_rx = reader(
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );
        let err_rx = reader(
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        );
        let deadline = cmd.timeout.map(|t| Instant::now() + t);
        let mut timed_out = false;
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut forwarded = false;
        let mut kill_at: Option<Instant> = None;
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Ok(st),
                Ok(None) => {}
                Err(e) => break Err(e),
            }
            #[cfg(unix)]
            if !forwarded && let Some(sig) = interrupted() {
                let name = if sig == signal_hook::consts::SIGINT {
                    "INT"
                } else {
                    "TERM"
                };
                signal_child(name, child.id(), grouped);
                forwarded = true;
                // Wait for the child below, bounded: it may ignore the signal.
                kill_at = Some(Instant::now() + Duration::from_secs(5));
            }
            if kill_at.is_some_and(|k| Instant::now() >= k) {
                #[cfg(unix)]
                signal_child("KILL", child.id(), grouped);
                let _ = child.kill();
                kill_at = None;
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                timed_out = true;
                #[cfg(unix)]
                signal_child("KILL", child.id(), grouped);
                let _ = child.kill();
                break child.wait();
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // The child is gone; a grandchild may still hold the pipes. One deadline covers both
        // pipes: after a kill 1 s, after a forwarded signal 2 s (keel must exit soon), after a
        // normal exit `PIPE_GRACE` (what the child wrote is already in the pipes). An interrupt
        // ends the wait at once.
        let until = Instant::now() + pipe_grace(timed_out, forwarded);
        type Drain = (
            std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
            mpsc::Receiver<()>,
        );
        let got = |(buf, rx): Drain| {
            let _ = wait_eof(&rx, until, &|| {
                interruptible_wait(forwarded) && interrupted().is_some()
            });
            buf.lock().map_or_else(
                |_| String::new(),
                |b| String::from_utf8_lossy(&b).into_owned(),
            )
        };
        let (stdout, mut stderr) = (got(out_rx), got(err_rx));
        match status {
            Ok(_) if timed_out => {
                let secs = cmd.timeout.map_or(0, |t| t.as_secs());
                if !stderr.is_empty() && !stderr.ends_with('\n') {
                    stderr.push('\n');
                }
                stderr.insert_str(0, &format!("timed out after {secs} s\n"));
                Out {
                    code: 124,
                    stdout,
                    stderr,
                }
            }
            Ok(st) => Out {
                code: st.code().unwrap_or(-1),
                stdout,
                stderr,
            },
            Err(e) => Out::with(-1, "", &e.to_string()),
        }
    }
}

/// A scripted runner for tests: matches by argv prefix (longest wins), records every call,
/// and answers an unknown command with code 127.
#[derive(Default)]
pub struct FakeRunner {
    rules: Vec<(Vec<String>, Out)>,
    calls: Mutex<Vec<Cmd>>,
}

impl FakeRunner {
    pub fn new() -> FakeRunner {
        FakeRunner::default()
    }

    pub fn on(mut self, prefix: &[&str], out: Out) -> FakeRunner {
        self.rules
            .push((prefix.iter().map(|s| s.to_string()).collect(), out));
        self
    }

    pub fn calls(&self) -> Vec<Cmd> {
        self.calls.lock().unwrap().clone()
    }
}

impl Runner for FakeRunner {
    fn run(&self, cmd: &Cmd) -> Out {
        self.calls.lock().unwrap().push(cmd.clone());
        let argv: Vec<&String> = std::iter::once(&cmd.program).chain(&cmd.args).collect();
        self.rules
            .iter()
            .filter(|(p, _)| p.len() <= argv.len() && p.iter().zip(&argv).all(|(a, b)| a == *b))
            .max_by_key(|(p, _)| p.len())
            .map(|(_, o)| o.clone())
            .unwrap_or_else(|| Out::with(127, "", &format!("fake: no rule for `{}`", cmd.line())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_longest_prefix_wins_and_records() {
        let r = FakeRunner::new()
            .on(&["gh"], Out::with(1, "", "generic"))
            .on(&["gh", "api", "user"], Out::ok("octo"));
        assert_eq!(
            r.run(&Cmd::new("gh", &["api", "user", "-q", ".login"]))
                .stdout,
            "octo"
        );
        assert_eq!(r.run(&Cmd::new("gh", &["auth", "status"])).code, 1);
        assert_eq!(r.calls().len(), 2);
        assert_eq!(r.calls()[0].args[0], "api");
    }

    #[test]
    fn fake_unknown_is_127() {
        let r = FakeRunner::new();
        assert_eq!(r.run(&Cmd::new("nope", &[])).code, 127);
    }

    #[test]
    fn debug_redacts_stdin() {
        let c = Cmd::new("gh", &["secret", "set"]).stdin(b"s3cret".to_vec());
        assert!(!format!("{c:?}").contains("s3cret"));
    }

    #[test]
    fn system_runner_not_found_is_minus_one() {
        let out = SystemRunner.run(&Cmd::new("keel-no-such-program-xyz", &[]));
        assert_eq!(out.code, -1);
    }

    #[test]
    fn system_runner_kills_a_child_after_the_timeout() {
        let t = Instant::now();
        let c = Cmd::new("sleep", &["5"]).timeout(Duration::from_secs(1));
        let out = SystemRunner.run(&c);
        assert!(t.elapsed() < Duration::from_secs(4), "{:?}", t.elapsed());
        assert_ne!(out.code, 0);
        assert!(out.stderr.contains("timed out after 1 s"), "{}", out.stderr);
        let line = crate::tools::failure(&c, &out);
        assert!(line.contains("timed out after 1 s"), "{line}");
        // A call that finishes in time is untouched, output and all.
        let c = Cmd::new("sh", &["-c", "echo hi; echo err >&2; exit 3"])
            .timeout(Duration::from_secs(5));
        let out = SystemRunner.run(&c);
        assert_eq!(
            (out.code, out.stdout.as_str(), out.stderr.as_str()),
            (3, "hi\n", "err\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_grandchild_holding_the_pipes_delays_an_untimed_call_by_one_bounded_grace() {
        // The child exits at once; its background grandchild keeps stdout and stderr open.
        // keel reads both pipes against ONE deadline, not one per pipe, and keeps the output.
        // A short grace for the test (the real one is `PIPE_GRACE`); the orphan is killed after.
        TEST_GRACE.set(Some(Duration::from_millis(300)));
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("orphan.pid");
        let script = format!("sleep 30 & echo $! > {}; echo hi", pidfile.display());
        let started = Instant::now();
        let out = SystemRunner.run(&Cmd::new("sh", &["-c", &script]));
        let took = started.elapsed();
        TEST_GRACE.set(None);
        if let Ok(pid) = std::fs::read_to_string(&pidfile) {
            let _ = Command::new("kill").args(["-9", pid.trim()]).status();
        }
        assert_eq!((out.code, out.stdout.as_str()), (0, "hi\n"));
        assert!(took >= Duration::from_millis(300), "{took:?}");
        assert!(
            took < Duration::from_millis(900),
            "one deadline, not two: {took:?}"
        );
    }

    /// After a forwarded signal keel is interrupted for good, yet the child's trailing output
    /// (written while it shuts down) is still read within the grace, not dropped at once.
    #[test]
    fn after_a_forwarded_signal_the_trailing_output_is_still_read() {
        let (tx, rx) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            let _ = tx.send(());
        });
        let started = Instant::now();
        let eof = wait_eof(&rx, Instant::now() + Duration::from_secs(2), &|| {
            interruptible_wait(true)
        });
        assert!(eof, "the trailing output's EOF arrived within the grace");
        assert!(started.elapsed() >= Duration::from_millis(150));
        // After a normal exit, an interrupt stops the wait at once.
        let (_tx, rx) = mpsc::channel::<()>();
        let started = Instant::now();
        let eof = wait_eof(&rx, Instant::now() + Duration::from_secs(2), &|| {
            interruptible_wait(false)
        });
        assert!(!eof);
        assert!(started.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn an_interrupt_ends_the_pipe_wait_at_once() {
        let (_tx, rx) = mpsc::channel::<()>();
        let started = Instant::now();
        let _ = wait_eof(&rx, Instant::now() + Duration::from_secs(10), &|| true);
        assert!(started.elapsed() < Duration::from_millis(200));
        // Not interrupted: it waits for the deadline.
        let started = Instant::now();
        let _ = wait_eof(&rx, Instant::now() + Duration::from_millis(150), &|| false);
        assert!(started.elapsed() >= Duration::from_millis(150));
    }

    #[test]
    fn the_pipe_grace_is_shortest_after_a_kill() {
        assert_eq!(pipe_grace(true, false), Duration::from_secs(1));
        assert_eq!(pipe_grace(false, true), Duration::from_secs(2));
        assert_eq!(pipe_grace(false, false), PIPE_GRACE);
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_the_whole_process_group() {
        // UC-WH-1: a script that spawns a child (flyctl under fly-setup.sh) must not leave it behind.
        // The liveness check reads /proc, which macOS lacks: skip there instead of passing vacuously.
        if !std::path::Path::new("/proc/self/stat").exists() {
            eprintln!("skip: no /proc");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("child.pid");
        let script = format!("sleep 30 & echo $! > {}; wait", pidfile.display());
        let c = Cmd::new("sh", &["-c", &script]).timeout(Duration::from_secs(1));
        let out = SystemRunner.run(&c);
        assert_eq!(out.code, 124, "{out:?}");
        let pid = std::fs::read_to_string(&pidfile)
            .unwrap()
            .trim()
            .to_string();
        let mut dead = false;
        for _ in 0..50 {
            // `kill -0` fails once the process is gone (a zombie still answers: check /proc state).
            // A missing /proc/<pid> means the process is gone; any other read error fails the test.
            let state = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => panic!("cannot read /proc/{pid}/stat: {e}"),
            };
            if state.is_empty() || state.contains(") Z") {
                dead = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(dead, "child {pid} survived the timeout");
    }

    #[test]
    fn fake_runner_records_the_timeout() {
        let r = FakeRunner::new().on(&["x"], Out::ok(""));
        r.run(&Cmd::new("x", &[]).timeout(Duration::from_secs(7)));
        r.run(&Cmd::new("x", &[]));
        let c = r.calls();
        assert_eq!(c[0].timeout, Some(Duration::from_secs(7)));
        assert_eq!(c[1].timeout, None);
    }

    #[test]
    fn system_runner_pipes_stdin_and_exit_code() {
        let c = Cmd::new("sh", &["-c", "cat; exit 4"]).stdin(b"hello".to_vec());
        let out = SystemRunner.run(&c);
        assert_eq!(out.code, 4);
        assert_eq!(out.stdout, "hello");
    }
}
