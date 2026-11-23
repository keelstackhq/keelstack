//! SessionStart: version skew, config errors, the janitor and the short rules block.
mod common;

use std::path::Path;
use std::process::Command;

use common::fake::FakeGithub;
use common::{bosun, run};
use serde_json::{Value, json};

fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(a)
            .output()
            .unwrap();
        assert!(o.status.success());
    };
    git(&["init", "-q", "-b", "main"]);
    git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/acme/widget.git",
    ]);
    // A set-up repo: session-start skips an un-adopted one.
    std::fs::create_dir_all(tmp.path().join(".bosun")).unwrap();
    std::fs::write(tmp.path().join(".bosun/version"), env!("CARGO_PKG_VERSION")).unwrap();
    tmp
}

fn start(api: &str, dir: &Path, now: &str) -> String {
    let out = bosun()
        .current_dir(dir)
        .env("BOSUN_SESSION", "session_me")
        .env("BOSUN_NOW", now)
        .env("BOSUN_GITHUB_API", api)
        .args(["hook", "session-start"])
        .write_stdin(json!({"cwd": dir.to_str().unwrap()}).to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let v: Value = common::one_object(&out.stdout);
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .to_string()
}

fn pin_file(dir: &Path, version: &str) {
    std::fs::create_dir_all(dir.join(".bosun")).unwrap();
    std::fs::write(dir.join(".bosun/version"), format!("{version}\n")).unwrap();
}

#[test]
fn a_binary_behind_the_repo_pin_says_how_to_fix_it_in_one_line() {
    let (fake, tmp) = (FakeGithub::start(), repo());
    pin_file(tmp.path(), "99.0.0");
    let want = format!(
        "installed {}, repo pins 99.0.0: run bash docs/setup-script.sh",
        env!("CARGO_PKG_VERSION")
    );
    for api in [fake.url(), "http://127.0.0.1:9".to_string()] {
        let text = start(&api, tmp.path(), "2026-10-22T10:00:00Z");
        assert!(text.contains(&want), "{text}");
    }
}

#[test]
fn a_matching_pin_is_silent() {
    let (fake, tmp) = (FakeGithub::start(), repo());
    pin_file(tmp.path(), env!("CARGO_PKG_VERSION"));
    let text = start(&fake.url(), tmp.path(), "2026-10-22T10:00:00Z");
    assert!(!text.contains("repo pins"), "{text}");
}

#[test]
fn an_unreleased_pin_is_silent() {
    // `unreleased` (spec §11): bosun is built from the workspace, no release to install.
    let (fake, tmp) = (FakeGithub::start(), repo());
    pin_file(tmp.path(), "unreleased");
    let text = start(&fake.url(), tmp.path(), "2026-10-22T10:00:00Z");
    assert!(!text.contains("repo pins"), "{text}");
}

#[test]
fn a_config_parse_error_is_named_not_blamed_on_github() {
    let tmp = repo();
    std::fs::create_dir_all(tmp.path().join(".bosun")).unwrap();
    std::fs::write(tmp.path().join(".bosun/bosun.toml"), "bogus_key = 1\n").unwrap();
    let text = start("http://127.0.0.1:9", tmp.path(), "2026-10-22T10:00:00Z");
    assert!(text.contains("bosun.toml"), "{text}");
    assert!(!text.contains("could not reach GitHub"), "{text}");
}

#[test]
fn session_start_reaps_expired_leases() {
    let fake = FakeGithub::start();
    let task = r#"x
<!-- bosun:task {"item":"M1","task":"T5","touch":["src/x/**"],"kind":"feature"} -->"#;
    fake.issue(5, "task", &["bosun:state/ready"], task);
    run(&fake, "session_a", "2026-10-22T10:00:00Z", &["join"]);
    run(&fake, "session_a", "2026-10-22T10:00:00Z", &["claim", "5"]);
    let tmp = repo();
    start(&fake.url(), tmp.path(), "2026-10-22T11:00:00Z");
    assert_eq!(fake.labels_of(5), vec!["bosun:state/ready"]);
}

#[test]
fn the_default_rules_stay_short() {
    let (fake, tmp) = (FakeGithub::start(), repo());
    let text = start(&fake.url(), tmp.path(), "2026-10-22T10:00:00Z");
    let rules = text.split("Session rules").nth(1).unwrap();
    assert!(rules.len() < 420, "{} chars: {rules}", rules.len());
}

/// An origin URL that starts with `-` is an option for git, not a repository: it is never passed
/// on (here `--upload-pack=touch MARK` would run `touch` against the local bare repo `HEAD`), and
/// the repo counts as adopted (fail closed).
#[test]
fn an_origin_url_starting_with_a_dash_is_never_given_to_git() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("work");
    let mark = tmp.path().join("MARK");
    std::fs::create_dir_all(&dir).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(a)
            .output()
            .unwrap();
        assert!(o.status.success(), "{a:?}: {o:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    // A local bare repo named `HEAD`: the repository the unprotected command would end up using.
    git(&["init", "-q", "--bare", "HEAD"]);
    let url = format!("--upload-pack=touch {}", mark.display());
    git(&["config", "remote.origin.url", &url]);
    let out = bosun()
        .current_dir(&dir)
        .env("BOSUN_SESSION", "session_me")
        .env("BOSUN_ADOPTED_CACHE_SECS", "0")
        .args(["hook", "session-start"])
        .write_stdin(json!({"cwd": dir.to_str().unwrap()}).to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(!mark.exists(), "git ran the URL as an option");
    let v: Value = common::one_object(&out.stdout);
    let text = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(!text.contains("not set up"), "{text}");
}

/// The adopted check asks the remote with a git that can never prompt: no ssh password or
/// passphrase prompt, no credential helper dialog, no terminal prompt.
#[test]
fn the_adopted_check_runs_git_non_interactively() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let (dir, tools) = (tmp.path().join("work"), tmp.path().join("tools"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&tools).unwrap();
    let real = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let log = tmp.path().join("git.log");
    let wrapper = tools.join("git");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho \"ssh=$GIT_SSH_COMMAND prompt=$GIT_TERMINAL_PROMPT args=$*\" >> '{}'\nexec {} \"$@\"\n",
            log.display(),
            real.trim()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new(real.trim())
            .arg("-C")
            .arg(&dir)
            .args(a)
            .output()
            .unwrap();
        assert!(o.status.success(), "{a:?}: {o:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["remote", "add", "origin", "git@github.com:acme/widget.git"]);
    let path = format!(
        "{}:{}",
        tools.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = bosun()
        .current_dir(&dir)
        .env("PATH", path)
        .env("BOSUN_SESSION", "session_me")
        .env("BOSUN_ADOPTED_CACHE_SECS", "0")
        .env_remove("GIT_SSH_COMMAND")
        .args(["hook", "session-start"])
        .write_stdin(json!({"cwd": dir.to_str().unwrap()}).to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let log = std::fs::read_to_string(&log).unwrap();
    let line = log
        .lines()
        .find(|l| l.contains("ls-remote"))
        .unwrap_or_else(|| panic!("{log}"));
    assert!(line.contains("ssh=ssh -o BatchMode=yes"), "{line}");
    assert!(line.contains("prompt=0"), "{line}");
    assert!(line.contains("credential.interactive=never"), "{line}");
}
