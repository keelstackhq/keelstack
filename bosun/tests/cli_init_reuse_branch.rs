//! `bosun init` on the current branch of a cloud session, with no token (PR through `gh` or the
//! MCP tool in `next`), and the owner from the authenticated user.
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::Value;

const OK_JSON: &str = r#"{"ok":true,"command":"channel add","changes":[{"path":".bosun/bosun.toml","action":"create","class":"shared"}],"messages":[]}"#;

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn exec(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Env {
    repo: tempfile::TempDir,
    origin: tempfile::TempDir,
    tools: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        let repo = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap();
        let tools = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(repo.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(repo.path().join("README.md"), "hi\n").unwrap();
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "x"]);
        git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        let e = Env {
            repo,
            origin,
            tools,
        };
        exec(
            &e.splice_path(),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nwhile [ $# -gt 0 ]; do [ \"$1\" = -C ] && d=\"$2\"; shift; done\nmkdir -p \"$d/.splice/channels\" \"$d/.bosun\"\necho '# x' > \"$d/.splice/channels/bosun.toml\"\necho '# x' > \"$d/.bosun/bosun.toml\"\necho '{OK_JSON}'\n",
                e.tools.path().join("argv.log").display()
            ),
        );
        e
    }

    fn splice_path(&self) -> PathBuf {
        self.tools.path().join("splice")
    }

    fn argv(&self) -> String {
        std::fs::read_to_string(self.tools.path().join("argv.log")).unwrap_or_default()
    }

    /// A fake `gh`: `auth token` fails, `auth status` and the rest follow the flags.
    fn gh(&self, status_ok: bool, login: &str, pr_url: &str) {
        exec(
            &self.tools.path().join("gh"),
            &format!(
                "#!/bin/sh\necho \"$@\" >> '{log}'\ncase \"$1 $2\" in\n\"auth token\") exit 1;;\n\"auth status\") exit {st};;\n\"api user\") echo '{login}';;\n\"pr create\") echo '{pr_url}';;\n\"label create\") echo ok;;\nesac\n",
                log = self.tools.path().join("gh.log").display(),
                st = if status_ok { 0 } else { 1 },
            ),
        );
    }

    fn gh_log(&self) -> String {
        std::fs::read_to_string(self.tools.path().join("gh.log")).unwrap_or_default()
    }

    fn run(&self, fake: &FakeGithub, token: bool, with_gh: bool, args: &[&str]) -> (i32, Value) {
        let mut cmd = bosun();
        let path = format!(
            "{}:{}",
            self.tools.path().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        cmd.env("BOSUN_GITHUB_API", fake.url())
            .env("BOSUN_SPLICE", self.splice_path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE", "cloud")
            .env("PATH", path);
        if !token {
            cmd.env_remove("GITHUB_TOKEN").env_remove("GH_TOKEN");
        }
        if with_gh {
            cmd.env_remove("BOSUN_NO_GH");
        }
        let out = cmd
            .arg("-C")
            .arg(self.repo.path())
            .arg("init")
            .args(["--stack", "rust"])
            .args(args)
            .output()
            .unwrap();
        let v = one_object(&out.stdout);
        if v["ok"] == false {
            assert!(!v["error"]["fix"].as_str().unwrap().trim().is_empty());
        }
        (out.status.code().unwrap(), v)
    }

    fn origin_has(&self, branch: &str) -> bool {
        Command::new("git")
            .arg("-C")
            .arg(self.origin.path())
            .args([
                "rev-parse",
                "--verify",
                "-q",
                &format!("refs/heads/{branch}"),
            ])
            .status()
            .unwrap()
            .success()
    }
}

#[test]
fn cloud_session_reuses_its_current_branch() {
    let e = Env::new();
    git(e.repo.path(), &["switch", "-q", "-c", "claude/work-abc"]);
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, true, false, &["--owner", "octocat"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["branch"], "claude/work-abc");
    assert_eq!(v["data"]["branch_reused"], true);
    assert_eq!(
        git_out(e.repo.path(), &["rev-parse", "--abbrev-ref", "HEAD"]),
        "claude/work-abc"
    );
    assert_eq!(
        git_out(e.repo.path(), &["log", "-1", "--format=%s"]),
        "chore: set up bosun (bosun init)"
    );
    assert!(e.origin_has("claude/work-abc"));
    assert!(!e.origin_has("claude/bosun-init"));
    let post = fake
        .requests()
        .into_iter()
        .find(|r| r.method == "POST" && r.path.ends_with("/pulls"))
        .expect("a PR was opened");
    assert!(post.body.unwrap().contains(r#""head":"claude/work-abc""#));
}

#[test]
fn explicit_branch_that_is_current_is_reused() {
    let e = Env::new();
    git(e.repo.path(), &["switch", "-q", "-c", "feature/x"]);
    let fake = FakeGithub::start();
    let (code, v) = e.run(
        &fake,
        true,
        false,
        &["--owner", "o", "--branch", "feature/x"],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["branch"], "feature/x");
    assert_eq!(v["data"]["branch_reused"], true);
}

#[test]
fn existing_branch_that_is_not_current_still_conflicts() {
    let e = Env::new();
    git(e.repo.path(), &["branch", "feature/x"]);
    let fake = FakeGithub::start();
    let (code, v) = e.run(
        &fake,
        true,
        false,
        &["--owner", "o", "--branch", "feature/x"],
    );
    assert_eq!(code, 3, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("already exists")
    );
}

#[test]
fn the_default_branch_is_never_reused() {
    let e = Env::new();
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, true, false, &["--owner", "o", "--branch", "main"]);
    assert_eq!(code, 3, "{v}");
    assert_eq!(git_out(e.repo.path(), &["log", "-1", "--format=%s"]), "x");
}

#[test]
fn a_cloud_branch_that_may_be_the_default_is_refused_when_the_default_is_unknown() {
    // No origin/HEAD and no main or master: the current branch may be the default branch.
    let e = Env::new();
    git(e.repo.path(), &["branch", "-m", "trunk"]);
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, true, false, &["--owner", "octocat"]);
    assert_eq!(code, 3, "{v}");
    assert_eq!(git_out(e.repo.path(), &["log", "-1", "--format=%s"]), "x");
    assert!(!e.origin_has("trunk"));
}

#[test]
fn without_a_token_the_pr_is_an_mcp_call_in_next() {
    let e = Env::new();
    git(e.repo.path(), &["switch", "-q", "-c", "claude/work-abc"]);
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, false, false, &["--owner", "octocat"]);
    assert_eq!(code, 0, "{v}");
    assert!(v["data"]["pr"].is_null());
    let next = v["next"].as_array().unwrap();
    assert_eq!(next.len(), 1, "{v}");
    assert_eq!(next[0]["tool"], "mcp__github__create_pull_request");
    let a = &next[0]["args"];
    assert_eq!(a["owner"], "acme");
    assert_eq!(a["repo"], "widget");
    assert_eq!(a["head"], "claude/work-abc");
    assert_eq!(a["base"], "main");
    assert_eq!(a["title"], "chore: set up bosun");
    assert_eq!(a["draft"], false);
    assert!(a["body"].as_str().unwrap().contains("R-gov"));
    let todo = v["data"]["labels_todo"].as_array().unwrap();
    assert!(
        todo.iter()
            .any(|c| c.as_str().unwrap().contains("gh label create bosun:halt"))
    );
    assert!(e.origin_has("claude/work-abc"));
    assert!(fake.requests().iter().all(|r| r.method != "POST"));
}

#[test]
fn without_a_token_an_authenticated_gh_opens_the_pr_and_labels() {
    let e = Env::new();
    git(e.repo.path(), &["switch", "-q", "-c", "claude/work-abc"]);
    e.gh(true, "ghuser", "https://github.com/acme/widget/pull/7");
    // `gh auth token` fails in the fake, so there is no API token: the PR goes through gh.
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, false, true, &["--owner", "octocat"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["pr"]["number"], 7);
    assert!(v["next"].as_array().unwrap().is_empty(), "{v}");
    let log = e.gh_log();
    assert!(
        log.contains("pr create") && log.contains("--head claude/work-abc"),
        "{log}"
    );
    assert!(log.contains("label create bosun:halt"), "{log}");
    assert!(
        v["data"]["labels_created"]
            .to_string()
            .contains("bosun:halt")
    );
    assert_eq!(v["data"]["labels_todo"].as_array().unwrap().len(), 0);
}

#[test]
fn owner_from_the_flag_the_api_or_gh() {
    // flag
    let e = Env::new();
    let fake = FakeGithub::start();
    fake.viewer("apiuser");
    let (code, v) = e.run(&fake, true, false, &["--owner", "octocat"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["owner"], "octocat");
    assert_eq!(v["data"]["owner_source"], "flag");
    // api
    let e = Env::new();
    let fake = FakeGithub::start();
    fake.viewer("apiuser");
    let (code, v) = e.run(&fake, true, false, &[]);
    assert_eq!(code, 0, "{v}");
    assert!(v["data"]["owner"].is_null(), "{v}");
    assert_eq!(v["data"]["owner_suggested"], "apiuser");
    assert_eq!(v["data"]["owner_source"], "api");
    assert!(!e.argv().contains("--data owner="), "{}", e.argv());
    assert!(
        v["data"]["owner_steps"]
            .to_string()
            .contains("Fill governance/owners.toml"),
        "{v}"
    );
    // gh
    let e = Env::new();
    e.gh(false, "ghuser", "");
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, false, true, &[]);
    assert_eq!(code, 0, "{v}");
    assert!(v["data"]["owner"].is_null(), "{v}");
    assert_eq!(v["data"]["owner_suggested"], "ghuser");
    assert_eq!(v["data"]["owner_source"], "gh");
    assert!(!e.argv().contains("--data owner="), "{}", e.argv());
    // none
    let e = Env::new();
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, true, false, &[]);
    assert_eq!(code, 0, "{v}");
    assert!(v["data"]["owner"].is_null());
    assert!(v["data"]["owner_source"].is_null());
    assert!(v["data"]["owner_suggested"].is_null());
}

#[test]
fn a_bot_or_agent_login_is_never_taken_as_owner() {
    for login in ["claude", "claude-agent", "renovate[bot]"] {
        let e = Env::new();
        let fake = FakeGithub::start();
        fake.viewer(login);
        let (code, v) = e.run(&fake, true, false, &[]);
        assert_eq!(code, 0, "{login}: {v}");
        assert!(v["data"]["owner"].is_null(), "{login}: {v}");
        assert!(v["data"]["owner_source"].is_null(), "{login}: {v}");
        assert!(v["data"]["owner_suggested"].is_null(), "{login}: {v}");
        assert!(
            v["data"]["owner_skipped"].as_str().unwrap().contains(login),
            "{login}: {v}"
        );
        assert!(v["data"]["owner_steps"].to_string().contains("owners.toml"));
        assert!(!e.argv().contains("--data owner="), "{}", e.argv());
    }
}

#[test]
fn an_inferred_login_is_suggested_never_written() {
    let suggested = |v: &Value, login: &str| {
        assert!(v["data"]["owner"].is_null(), "{v}");
        assert_eq!(v["data"]["owner_suggested"], login, "{v}");
        let steps = v["data"]["owner_steps"].to_string();
        assert!(
            steps.contains(&format!(
                "Fill governance/owners.toml with your GitHub login (suggested: {login}), or rerun `bosun init --owner {login}`"
            )),
            "{v}"
        );
        assert!(!steps.contains("Confirm"), "{v}");
    };
    let e = Env::new();
    let fake = FakeGithub::start();
    fake.viewer("apiuser");
    let (code, v) = e.run(&fake, true, false, &[]);
    assert_eq!(code, 0, "{v}");
    suggested(&v, "apiuser");
    assert!(!e.argv().contains("--data owner="), "{}", e.argv());

    let e = Env::new();
    e.gh(false, "ghuser", "");
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, false, true, &[]);
    assert_eq!(code, 0, "{v}");
    suggested(&v, "ghuser");

    // The owner named by hand is written and needs no step.
    let e = Env::new();
    let fake = FakeGithub::start();
    fake.viewer("apiuser");
    let (code, v) = e.run(&fake, true, false, &["--owner", "octocat"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["owner"], "octocat");
    assert!(v["data"]["owner_suggested"].is_null(), "{v}");
    assert!(!v["data"]["owner_steps"].to_string().contains("owners.toml"));
}

#[test]
fn a_human_login_that_starts_like_an_agent_is_accepted_as_owner() {
    let e = Env::new();
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, true, false, &["--owner", "claudia-x"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["owner"], "claudia-x", "{v}");
}

#[test]
fn an_agent_login_passed_as_owner_is_refused() {
    for login in ["claude", "Claude-Agent", "claude-code", "renovate[bot]"] {
        let e = Env::new();
        let fake = FakeGithub::start();
        let (code, v) = e.run(&fake, true, false, &["--owner", login]);
        assert_eq!(code, 2, "{login}: {v}");
        assert_eq!(v["ok"], false, "{login}: {v}");
        assert!(
            v["error"]["fix"]
                .as_str()
                .unwrap()
                .contains("pass the human owner's GitHub login: `--owner <login>`"),
            "{login}: {v}"
        );
    }
}

#[test]
fn a_failed_label_create_keeps_the_kill_switch_owner_step() {
    let e = Env::new();
    git(e.repo.path(), &["switch", "-q", "-c", "claude/work-abc"]);
    // `gh label create` fails: the labels stay in `labels_todo`, so the owner step stays too.
    exec(
        &e.tools.path().join("gh"),
        "#!/bin/sh\ncase \"$1 $2\" in\n\"auth token\") exit 1;;\n\"auth status\") exit 0;;\n\"api user\") echo ghuser;;\n\"pr create\") echo https://github.com/acme/widget/pull/7;;\n\"label create\") exit 1;;\nesac\n",
    );
    // A parallel test's fork can hold the script's write fd for a moment (ETXTBSY): wait until
    // the stub runs, so bosun never meets a `gh` it cannot execute (or falls through to a real one).
    for _ in 0..100 {
        if std::process::Command::new(e.tools.path().join("gh"))
            .arg("probe")
            .output()
            .is_ok()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, false, true, &["--owner", "octocat"]);
    assert_eq!(code, 0, "{v}");
    assert!(
        !v["data"]["labels_todo"].as_array().unwrap().is_empty(),
        "{v}"
    );
    assert!(
        v["data"]["owner_steps"].to_string().contains("bosun:halt"),
        "{v}"
    );
}

/// A reused branch with a tracked `.gitignore` and an ignored file, ready for a failing init.
fn reused_with_ignored(e: &Env) -> String {
    std::fs::write(e.repo.path().join(".gitignore"), "keep.log\n").unwrap();
    git(e.repo.path(), &["add", "-A"]);
    git(e.repo.path(), &["commit", "-q", "-m", "ignore"]);
    git(e.repo.path(), &["switch", "-q", "-c", "claude/work-abc"]);
    std::fs::write(e.repo.path().join("keep.log"), "mine\n").unwrap();
    git_out(e.repo.path(), &["rev-parse", "HEAD"])
}

fn assert_restored(e: &Env, head: &str, v: &Value) {
    assert_eq!(git_out(e.repo.path(), &["rev-parse", "HEAD"]), head, "{v}");
    assert_eq!(
        git_out(e.repo.path(), &["rev-parse", "--abbrev-ref", "HEAD"]),
        "claude/work-abc"
    );
    // The files init created are gone, the ignored file is still there.
    assert!(!e.repo.path().join(".bosun").exists(), "{v}");
    assert!(!e.repo.path().join(".splice").exists(), "{v}");
    assert_eq!(
        std::fs::read_to_string(e.repo.path().join("keep.log")).unwrap(),
        "mine\n"
    );
    assert_eq!(git_out(e.repo.path(), &["status", "--porcelain"]), "");
    let fix = v["error"]["fix"].as_str().unwrap();
    assert!(
        fix.contains(&format!("reset claude/work-abc to {head}")),
        "{fix}"
    );
}

#[test]
fn a_failed_splice_on_a_reused_branch_restores_head_and_keeps_ignored_files() {
    let e = Env::new();
    let head = reused_with_ignored(&e);
    exec(
        &e.splice_path(),
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do [ \"$1\" = -C ] && d=\"$2\"; shift; done\nmkdir -p \"$d/.bosun\" \"$d/.splice\"\necho x > \"$d/.bosun/partial.toml\"\necho x > \"$d/.splice/partial.toml\"\necho '{\"ok\":false,\"messages\":[\"boom\"]}'\nexit 1\n",
    );
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, true, false, &["--owner", "octocat"]);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["message"].as_str().unwrap().contains("boom"),
        "{v}"
    );
    assert_restored(&e, &head, &v);
    assert!(
        v["error"]["fix"]
            .as_str()
            .unwrap()
            .contains("fix the splice error above"),
        "{v}"
    );
}

#[test]
fn a_failed_push_on_a_reused_branch_restores_head_and_keeps_ignored_files() {
    let e = Env::new();
    let head = reused_with_ignored(&e);
    git(
        e.repo.path(),
        &["remote", "set-url", "origin", "/nonexistent/origin.git"],
    );
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, true, false, &["--owner", "octocat"]);
    assert_ne!(code, 0, "{v}");
    assert_eq!(v["ok"], false, "{v}");
    assert_restored(&e, &head, &v);
}

#[test]
fn an_explicit_branch_is_not_reused_when_the_default_is_unknown() {
    // No origin/HEAD and no main or master, no token: the current branch may be the default.
    let e = Env::new();
    git(e.repo.path(), &["branch", "-m", "trunk"]);
    let fake = FakeGithub::start();
    let (code, v) = e.run(
        &fake,
        false,
        false,
        &["--owner", "o", "--branch", "trunk", "--no-pr"],
    );
    assert_eq!(code, 3, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cannot tell whether"),
        "{v}"
    );
    assert_eq!(git_out(e.repo.path(), &["log", "-1", "--format=%s"]), "x");
    assert!(!e.origin_has("trunk"));
}

#[test]
fn an_existing_label_is_never_overwritten_with_force() {
    let e = Env::new();
    git(e.repo.path(), &["switch", "-q", "-c", "claude/work-abc"]);
    exec(
        &e.tools.path().join("gh"),
        &format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\ncase \"$1 $2\" in\n\"auth token\") exit 1;;\n\"auth status\") exit 0;;\n\"api user\") echo ghuser;;\n\"pr create\") echo https://github.com/acme/widget/pull/7;;\n\"label list\") echo bosun:halt;;\n\"label create\") echo ok;;\nesac\n",
            e.tools.path().join("gh.log").display()
        ),
    );
    for _ in 0..100 {
        if Command::new(e.tools.path().join("gh"))
            .arg("probe")
            .output()
            .is_ok()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = std::fs::remove_file(e.tools.path().join("gh.log"));
    let fake = FakeGithub::start();
    let (code, v) = e.run(&fake, false, true, &["--owner", "octocat"]);
    assert_eq!(code, 0, "{v}");
    let log = e.gh_log();
    assert!(!log.contains("--force"), "{log}");
    assert!(!log.contains("label create bosun:halt"), "{log}");
    assert!(log.contains("label create"), "{log}");
    assert!(
        !v["data"]["labels_created"]
            .to_string()
            .contains("bosun:halt"),
        "{v}"
    );
    assert_eq!(v["data"]["labels_todo"].as_array().unwrap().len(), 0, "{v}");
}
