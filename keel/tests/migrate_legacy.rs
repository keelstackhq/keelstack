//! `keel migrate` on a repo whose splice lock still names a legacy `gh:keelstackhq/<tool>` source:
//! `splice update` repoints it first (its own commit); keel reports that as the `repoint` step,
//! before the `splice` step, and the run ends changed at the target instead of "up to date".
//! A stub `splice` on PATH scripts the repoint (no network, no real tool).
use assert_cmd::Command;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

const STUB: &str = r#"#!/bin/sh
case "$*" in
  "--version") echo "splice $KEEL_TARGET";;
  "--json check") echo '{"ok":true,"exit_code":0}';;
  "--json --yes update --all")
    if grep -q 'gh:keelstackhq/hull//template"' .splice/lock.toml; then
      sed -i 's#gh:keelstackhq/hull//template#gh:keelstackhq/keelstack//hull/template#' .splice/lock.toml
      git add -A && git commit -q -m "chore(splice): repoint gh:keelstackhq/hull//template to the monorepo"
      echo '{"ok":true,"changes":[],"messages":["repointed gh:keelstackhq/hull//template to gh:keelstackhq/keelstack//hull/template at abc1234 (.splice/lock.toml)"]}'
    else
      echo '{"ok":true,"changes":[],"messages":[]}'
    fi;;
esac
exit 0
"#;

fn git(dir: &Path, args: &[&str]) {
    let o = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

fn run(source: &str) -> (i32, serde_json::Value) {
    let root = tempfile::tempdir().unwrap();
    let mk = |n: &str| {
        let d = root.path().join(n);
        std::fs::create_dir(&d).unwrap();
        d
    };
    let (stubs, repo, remote) = (mk("stubs"), mk("repo"), mk("remote.git"));
    let script = stubs.join("splice");
    std::fs::write(&script, STUB).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.path().join("pins.toml"), "[pins]\n").unwrap();
    git(&remote, &["init", "--bare", "-q", "-b", "main"]);
    git(&repo, &["init", "-q", "-b", "main"]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    std::fs::create_dir_all(repo.join(".splice")).unwrap();
    std::fs::write(
        repo.join(".splice/lock.toml"),
        format!("[template]\nsource = \"{source}\"\nversion = \"0.13.0\"\n"),
    )
    .unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    let target = env!("CARGO_PKG_VERSION");
    let o = Command::cargo_bin("keel")
        .unwrap()
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
        .env("HOME", root.path())
        .env("TMPDIR", root.path())
        .env("KEEL_TARGET", target)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("KEEL_PINS", root.path().join("pins.toml"))
        .arg("-C")
        .arg(&repo)
        .args(["--json", "migrate", "--to", target])
        .output()
        .unwrap();
    let v = serde_json::from_slice(&o.stdout).expect("one JSON object");
    (o.status.code().unwrap(), v)
}

fn ids(v: &serde_json::Value) -> Vec<String> {
    v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_legacy_source_is_repointed_as_its_own_step_and_the_run_is_done_not_up_to_date() {
    let (code, v) = run("gh:keelstackhq/hull//template");
    // Changed, so keel's own `open-pr` task is the only thing left (no --pr): not "up to date".
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["checklist"][0]["id"], "open-pr", "{v}");
    assert_eq!(v["changed"], true, "{v}");
    let ids = ids(&v);
    let (rp, sp) = (
        ids.iter()
            .position(|i| i == "repoint")
            .expect("repoint step"),
        ids.iter().position(|i| i == "splice").unwrap(),
    );
    assert_eq!(rp + 1, sp, "repoint right before splice: {ids:?}");
    let s = &v["steps"][rp];
    assert_eq!(s["action"], "done", "{v}");
    assert!(
        s["detail"]
            .as_str()
            .unwrap()
            .contains("repointed gh:keelstackhq/hull//template to gh:keelstackhq/keelstack//hull/template at abc1234"),
        "{s}"
    );
}

#[test]
fn a_repo_with_no_legacy_source_has_a_skipped_repoint_step() {
    let (code, v) = run("gh:keelstackhq/keelstack//hull/template");
    assert_eq!(code, 0, "{v}");
    let ids = ids(&v);
    let rp = ids
        .iter()
        .position(|i| i == "repoint")
        .expect("repoint step");
    assert_eq!(v["steps"][rp]["action"], "skipped", "{v}");
    assert_eq!(v["changed"], false, "{v}");
}
