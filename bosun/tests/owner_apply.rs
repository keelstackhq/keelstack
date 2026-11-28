//! `bosun owner setup` (B3 T2): after the push it waits for PR A to merge, then runs
//! `hoist apply --area github --yes` (a stub hoist on PATH), against the fake GitHub.

mod common;

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::Value;

const KEY: &str = "-----BEGIN RSA PRIVATE KEY-----\nKEYMATERIALCANARY0123456789abcdef\n-----END RSA PRIVATE KEY-----\n";
const MANUAL: [&str; 10] = [
    "owner",
    "setup",
    "--manual",
    "--app-id",
    "777",
    "--client-id",
    "Iv1.fakeclient",
    "--slug",
    "acme-bosun-gate",
    "--key-stdin",
];

fn git(dir: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

struct Env {
    fake: FakeGithub,
    repo: tempfile::TempDir,
    _origin: tempfile::TempDir,
    home: tempfile::TempDir,
    bin: tempfile::TempDir,
}

impl Env {
    /// `hoist_exit`: the stub hoist writes its argv to `$HOME/hoist.args` and exits with it.
    fn new(hoist_exit: i32) -> Env {
        let fake = FakeGithub::start();
        fake.state.lock().unwrap().manifest_pem = KEY.to_string();
        let repo = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(repo.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(repo.path().join("README.md"), "hi\n").unwrap();
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "init"]);
        let o = origin.path().to_str().unwrap();
        git(repo.path(), &["remote", "add", "origin", o]);
        git(repo.path(), &["push", "-q", "origin", "main"]);
        let bin = tempfile::tempdir().unwrap();
        let stub = bin.path().join("hoist");
        let mut f = std::fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\necho \"$@\" >> \"$HOME/hoist.args\"\necho '{{\"ok\":true}}'\nexit {hoist_exit}"
        )
        .unwrap();
        drop(f);
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        Env {
            fake,
            repo,
            _origin: origin,
            home: tempfile::tempdir().unwrap(),
            bin,
        }
    }

    fn run(&self, args: &[&str], wait: &str) -> (std::process::Output, Value) {
        let path = format!(
            "{}:{}",
            self.bin.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let out = bosun()
            .current_dir(self.repo.path())
            .env("BOSUN_GITHUB_API", self.fake.url())
            .env("HOME", self.home.path())
            .env("PATH", path)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("BOSUN_OWNER_WAIT_SECS", wait)
            .env("BOSUN_OWNER_POLL_MS", "20")
            .args(args)
            .write_stdin(KEY)
            .output()
            .unwrap();
        let v = one_object(&out.stdout);
        (out, v)
    }

    fn hoist_calls(&self) -> String {
        std::fs::read_to_string(self.home.path().join("hoist.args")).unwrap_or_default()
    }
}

fn status<'a>(v: &'a Value, step: &str) -> &'a str {
    v["data"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == step)
        .unwrap_or_else(|| panic!("no step {step}: {v}"))["status"]
        .as_str()
        .unwrap()
}

#[test]
fn merged_pr_a_then_hoist_apply_runs_once_with_the_github_area() {
    let e = Env::new(0);
    e.fake.pull(5, "gate app", "bosun/gate-app", "");
    e.fake.pull_merged(5, "mergesha");
    // Lists carry `merged_at`, not `merged`.
    e.fake
        .pull_set(5, "merged_at", serde_json::json!("2026-11-27T10:00:00Z"));
    let (out, v) = e.run(&MANUAL, "5");
    assert!(out.status.success(), "{v}");
    assert_eq!(status(&v, "wait-pr-a"), "done", "{v}");
    assert_eq!(status(&v, "hoist-apply"), "done", "{v}");
    let calls = e.hoist_calls();
    assert_eq!(calls.lines().count(), 1, "{calls}");
    assert!(calls.contains("apply --area github --yes"), "{calls}");
    assert!(!calls.contains("PRIVATE KEY"), "{calls}");
}

#[test]
fn pr_a_merging_during_the_wait_ends_it() {
    let e = Env::new(0);
    e.fake.pull(5, "gate app", "bosun/gate-app", "");
    let url = e.fake.state.clone();
    let merger = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        let mut s = url.lock().unwrap();
        s.pulls[0]["merged"] = serde_json::json!(true);
        s.pulls[0]["state"] = serde_json::json!("closed");
        s.pulls[0]["merged_at"] = serde_json::json!("2026-11-27T10:00:00Z");
    });
    let (out, v) = e.run(&MANUAL, "20");
    merger.join().unwrap();
    assert!(out.status.success(), "{v}");
    assert_eq!(status(&v, "hoist-apply"), "done", "{v}");
    assert_eq!(e.hoist_calls().lines().count(), 1);
}

#[test]
fn an_unmerged_pr_a_times_out_without_running_hoist() {
    let e = Env::new(0);
    e.fake.pull(5, "gate app", "bosun/gate-app", "");
    let (out, v) = e.run(&MANUAL, "1");
    assert!(out.status.success(), "{v}");
    assert_eq!(status(&v, "wait-pr-a"), "pending", "{v}");
    assert_eq!(status(&v, "hoist-apply"), "pending", "{v}");
    assert_eq!(e.hoist_calls(), "");
}

#[test]
fn a_failing_hoist_is_an_error_with_a_fix() {
    let e = Env::new(3);
    e.fake.pull(5, "gate app", "bosun/gate-app", "");
    e.fake.pull_merged(5, "mergesha");
    // Lists carry `merged_at`, not `merged`.
    e.fake
        .pull_set(5, "merged_at", serde_json::json!("2026-11-27T10:00:00Z"));
    let (out, v) = e.run(&MANUAL, "5");
    assert_eq!(out.status.code(), Some(1), "{v}");
    assert!(
        v["error"]["fix"]
            .as_str()
            .unwrap()
            .contains("hoist apply --area github")
    );
}

#[test]
fn dry_run_lists_both_steps_and_runs_nothing() {
    let e = Env::new(0);
    let (out, v) = e.run(&["owner", "setup", "--dry-run"], "5");
    assert!(out.status.success(), "{v}");
    assert_eq!(status(&v, "wait-pr-a"), "would-do");
    assert_eq!(status(&v, "hoist-apply"), "would-do");
    assert_eq!(e.hoist_calls(), "");
    let muts: Vec<_> = e
        .fake
        .requests()
        .into_iter()
        .filter(|r| r.method != "GET")
        .collect();
    assert!(muts.is_empty(), "{muts:?}");
}
