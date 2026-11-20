use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
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
        // Both pipes are drained on threads so a chatty child never blocks on a full pipe.
        let reader = |pipe: Option<Box<dyn Read + Send>>| {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut p) = pipe {
                    let _ = p.read_to_end(&mut buf);
                }
                let _ = tx.send(buf);
            });
            rx
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
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Ok(st),
                Ok(None) => {}
                Err(e) => break Err(e),
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                timed_out = true;
                let _ = child.kill();
                break child.wait();
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        // After a kill a grandchild may still hold the pipes: wait only briefly for them.
        let grace = if timed_out {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(3600)
        };
        let got = |rx: mpsc::Receiver<Vec<u8>>| {
            String::from_utf8_lossy(&rx.recv_timeout(grace).unwrap_or_default()).into_owned()
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
