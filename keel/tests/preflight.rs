//! `keel preflight` and the preflight gate of `up`, `plan` and `migrate`, against stub `gh`/`git`
//! scripts first on PATH (no real tool, no network). A block exits 5 before any write.
use assert_cmd::Command;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

const STUB: &str = r#"#!/bin/sh
name=$(basename "$0")
echo "$name $*" >> "$KEEL_LOG/calls.log"
case "$name $*" in
  "gh auth status"*) echo "  - Token scopes: 'repo', 'workflow', 'read:org'";;
  "gh api user --jq .login") echo octo;;
  "gh api repos/owner/name")
    echo "{\"private\":$KEEL_PRIVATE,\"owner\":{\"login\":\"owner\",\"type\":\"Organization\"},\"permissions\":{\"admin\":true}}";;
  "gh api orgs/owner") echo "{\"plan\":{\"name\":\"$KEEL_PLAN\"}}";;
  "git --version") echo "git version 2.47.0";;
  "node --version") echo v24.1.0;;
  "gh repo view"*) echo '{"defaultBranchRef":{"name":"main"}}';;
esac
exit 0
"#;

struct Env {
    _root: tempfile::TempDir,
    stubs: PathBuf,
    log: PathBuf,
    repo: PathBuf,
    pins: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let o = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}");
}

impl Env {
    fn new() -> Env {
        let root = tempfile::tempdir().unwrap();
        let p = |n: &str| {
            let d = root.path().join(n);
            std::fs::create_dir(&d).unwrap();
            d
        };
        let (stubs, log, repo) = (p("stubs"), p("log"), p("repo"));
        let script = stubs.join("stub.sh");
        std::fs::write(&script, STUB).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        for t in ["gh", "node"] {
            symlink(&script, stubs.join(t)).unwrap();
        }
        let pins = root.path().join("pins.toml");
        std::fs::write(&pins, "[pins]\n").unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/owner/name.git",
            ],
        );
        Env {
            _root: root,
            stubs,
            log,
            repo,
            pins,
        }
    }

    fn keel(&self, private: &str, plan: &str, args: &[&str]) -> Command {
        let mut c = Command::cargo_bin("keel").unwrap();
        c.env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.stubs.display()))
            .env("HOME", &self.log)
            .env("TMPDIR", &self.log)
            .env("KEEL_LOG", &self.log)
            .env("KEEL_PRIVATE", private)
            .env("KEEL_PLAN", plan)
            .env("KEEL_PINS", &self.pins)
            .arg("-C")
            .arg(&self.repo)
            .args(args);
        c
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.log.join("calls.log")).unwrap_or_default()
    }

    /// Nothing was written: no keel.toml, no migration branch, no worktree, no tool call.
    fn assert_untouched(&self) {
        assert!(!self.repo.join("keel.toml").exists());
        let branches = std::process::Command::new("git")
            .args(["branch", "--list", "keel/*"])
            .current_dir(&self.repo)
            .output()
            .unwrap();
        assert!(branches.stdout.is_empty());
        let calls = self.calls();
        for bad in ["git fetch", "npm ", "splice ", "gh pr create", "gh secret"] {
            assert!(!calls.contains(bad), "{bad:?} in {calls}");
        }
    }
}

fn stdout(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8(a.get_output().stdout.clone()).unwrap()
}

const UP: [&str; 7] = [
    "up",
    "--repo",
    "owner/name",
    "--template",
    "rust",
    "--pieces",
    "rust-cli:x",
];

#[test]
fn private_repo_on_free_org_blocks_every_command_with_the_three_choices() {
    for args in [
        vec!["migrate"],
        UP.to_vec(),
        vec!["plan", "--repo", "owner/name", "--template", "rust"],
    ] {
        let env = Env::new();
        let mut full = vec!["--json"];
        full.extend(&args);
        let a = env.keel("true", "free", &full).assert().code(5);
        let out = stdout(&a);
        let v: serde_json::Value = serde_json::from_str(out.trim()).expect("one JSON object");
        assert_eq!(v["exit_code"], 5);
        assert_eq!(v["command"], args[0]);
        let f = &v["findings"][0];
        assert_eq!(f["severity"], "block");
        assert_eq!(f["id"], "plan-free-private");
        let fix = f["fix"].as_str().unwrap();
        for choice in ["upgrade", "public", "[ci] provider = \"none\""] {
            assert!(fix.contains(choice), "{choice}: {fix}");
        }
        assert!(f["device"].is_string() && f["message"].is_string());
        env.assert_untouched();
    }
}

#[test]
fn preflight_command_blocks_and_writes_nothing() {
    let env = Env::new();
    let a = env.keel("true", "free", &["preflight"]).assert().code(5);
    assert!(stdout(&a).contains("plan-free-private"));
    env.assert_untouched();
}

#[test]
fn public_repo_on_free_org_passes_and_warns_only_about_legacy_sources() {
    let env = Env::new();
    std::fs::create_dir(env.repo.join(".splice")).unwrap();
    std::fs::write(
        env.repo.join(".splice/lock.toml"),
        "source = \"gh:keelstackhq/hull//template-rust\"\n",
    )
    .unwrap();
    let a = env
        .keel("false", "free", &["--json", "preflight"])
        .assert()
        .code(0);
    let out = stdout(&a);
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect("one JSON object");
    assert_eq!(v["command"], "preflight");
    assert_eq!(v["ok"], true);
    let f = v["findings"].as_array().unwrap();
    assert!(f.iter().all(|x| x["severity"] == "warn"), "{f:?}");
    assert!(f.iter().any(|x| x["id"] == "legacy-source"), "{f:?}");
    assert!(!out.contains("ghp_"));
    env.assert_untouched();
}

#[test]
fn private_repo_on_a_paid_org_is_clean() {
    let env = Env::new();
    let a = env
        .keel("true", "team", &["--json", "preflight"])
        .assert()
        .code(0);
    let v: serde_json::Value = serde_json::from_str(stdout(&a).trim()).unwrap();
    assert_eq!(v["findings"].as_array().unwrap().len(), 0);
}

#[test]
fn missing_repo_scope_blocks() {
    let env = Env::new();
    // The token has no `repo` scope.
    let script = std::fs::read_to_string(env.stubs.join("stub.sh")).unwrap();
    std::fs::write(env.stubs.join("stub.sh"), script.replace("'repo', ", "")).unwrap();
    let a = env
        .keel("false", "free", &["--json", "preflight"])
        .assert()
        .code(5);
    assert!(stdout(&a).contains("scope-repo"));
}
