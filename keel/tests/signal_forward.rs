//! SIGINT/SIGTERM sent to keel alone must stop its running child (no orphan).
#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn check(sig: &str) {
    check_with(sig, false);
}

fn check_with(sig: &str, grandchild: bool) {
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pid");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let gpid = dir.path().join("gpid");
    // `grandchild`: the child dies on the signal but a `sleep 20` it started keeps stdout open.
    let tail = if grandchild {
        format!("sleep 20 &\necho $! > {}\nwait", gpid.display())
    } else {
        "exec sleep 30".to_string()
    };
    for tool in ["git", "node", "gh"] {
        let p = bin.join(tool);
        let body = format!("#!/bin/sh\necho $$ > {}\n{tail}\n", pidfile.display());
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut keel = Command::new(env!("CARGO_BIN_EXE_keel"))
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--json",
            "plan",
            "--repo",
            "owner/name",
            "--template",
            "rust",
            "--pieces",
            "rust-cli:my-cli",
            "--wait=0",
        ])
        .env("HOME", dir.path())
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let t = Instant::now();
    while !pidfile.exists() || std::fs::read_to_string(&pidfile).unwrap().trim().is_empty() {
        assert!(t.elapsed() < Duration::from_secs(10), "child never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    let pid = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .to_string();
    assert!(alive(&pid));
    Command::new("kill")
        .args([&format!("-{sig}"), &keel.id().to_string()])
        .status()
        .unwrap();
    let sent = Instant::now();
    let status = keel.wait().unwrap();
    if grandchild && let Ok(g) = std::fs::read_to_string(&gpid) {
        // Do not leave the `sleep 20` behind.
        let _ = Command::new("kill").arg(g.trim()).status();
    }
    if grandchild {
        assert!(
            sent.elapsed() < Duration::from_secs(8),
            "keel waited for the grandchild"
        );
    }
    // The signal stops keel through the normal error path: exit 128 + signal, not a raw kill.
    let want = if sig == "INT" { 130 } else { 143 };
    assert_eq!(status.code(), Some(want), "exit code after {sig}");
    let t = Instant::now();
    while alive(&pid) {
        assert!(
            t.elapsed() < Duration::from_secs(5),
            "child orphaned after {sig}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn sigterm_stops_child() {
    check("TERM");
}

/// A grandchild that outlives the signalled child and holds stdout open must not delay exit.
#[test]
fn sigterm_does_not_wait_for_grandchild_pipe() {
    check_with("TERM", true);
}

#[test]
fn sigint_stops_child() {
    check("INT");
}

/// A signal with no child running (the `--wait` poll sleep) stops keel promptly with the
/// interrupted exit code and its `--json` error object, after an earlier subprocess finished.
#[test]
fn sigint_during_wait_poll_exits_130_with_json() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("polled");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let stub = format!(
        r#"#!/bin/sh
case "$(basename "$0") $*" in
  "node --version") echo v24.1.0;;
  "git --version") echo "git version 2.47.0";;
  "git rev-parse --is-inside-work-tree") echo true;;
  "git remote get-url origin") echo https://github.com/owner/name.git;;
  "git rev-parse --abbrev-ref HEAD") echo keel/bootstrap;;
  "gh auth status"*) ;;
  "gh repo view"*) echo '{{"defaultBranchRef":{{"name":"main"}}}}';;
  "splice --version") echo "splice 0.23.0";;
  "bosun --version") echo "bosun 0.23.0";;
  "hoist --version") echo "hoist 0.23.0";;
  "berth --version") echo "berth 0.23.0";;
  "gh api user --jq .login") echo octo;;
  "gh api user/installations --paginate"*)
    echo '{{"id":1,"app_slug":"claude","repository_selection":"all"}}'
    : > {marker};;
esac
exit 0
"#,
        marker = marker.display()
    );
    for tool in ["git", "node", "gh", "splice", "bosun", "hoist", "berth"] {
        let p = bin.join(tool);
        std::fs::write(&p, &stub).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = std::fs::File::create(dir.path().join("out.json")).unwrap();
    let mut keel = Command::new(env!("CARGO_BIN_EXE_keel"))
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--json",
            "up",
            "--repo",
            "owner/name",
            "--template",
            "rust",
            "--pieces",
            "rust-cli:my-cli",
            "--wait=600",
        ])
        .env("HOME", dir.path())
        .env("KEEL_POLL_SECS", "300")
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let t = Instant::now();
    while !marker.exists() {
        assert!(t.elapsed() < Duration::from_secs(10), "poll never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Let the subprocess finish and the poll sleep begin (the flag is checked either way).
    std::thread::sleep(Duration::from_millis(500));
    let sent = Instant::now();
    Command::new("kill")
        .args(["-INT", &keel.id().to_string()])
        .status()
        .unwrap();
    let status = loop {
        if let Some(st) = keel.try_wait().unwrap() {
            break st;
        }
        if sent.elapsed() > Duration::from_secs(5) {
            let _ = keel.kill();
            panic!("keel ignored SIGINT during the --wait poll");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(130));
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("out.json")).unwrap())
            .unwrap();
    assert_eq!(json["exit_code"], 130);
    assert_eq!(json["ok"], false);
}
