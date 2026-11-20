use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use zeroize::Zeroizing;

/// One subprocess call. `stdin` carries secrets: it is never printed (see the `Debug` impl).
#[derive(Clone)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub dir: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<Zeroizing<Vec<u8>>>,
}

impl Cmd {
    pub fn new(program: &str, args: &[&str]) -> Cmd {
        Cmd {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            dir: None,
            env: Vec::new(),
            stdin: None,
        }
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
        match child.wait_with_output() {
            Ok(o) => Out {
                code: o.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
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
    fn system_runner_pipes_stdin_and_exit_code() {
        let c = Cmd::new("sh", &["-c", "cat; exit 4"]).stdin(b"hello".to_vec());
        let out = SystemRunner.run(&c);
        assert_eq!(out.code, 4);
        assert_eq!(out.stdout, "hello");
    }
}
