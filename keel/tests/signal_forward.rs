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
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pid");
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    for tool in ["git", "node", "gh"] {
        let p = bin.join(tool);
        let body = format!(
            "#!/bin/sh\necho $$ > {}\nexec sleep 30\n",
            pidfile.display()
        );
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
    keel.wait().unwrap();
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

#[test]
fn sigint_stops_child() {
    check("INT");
}
