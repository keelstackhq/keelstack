//! Legacy `gh:keelstackhq/<tool>//<dir>` sources repoint to the monorepo on `splice update`
//! (UC-S1-1, UC-S1-2). Offline: git's `insteadOf` maps github.com/keelstackhq/ to a local dir.

mod common;

use common::*;
use std::path::Path;
use tempfile::TempDir;

const DATE: &str = "2026-01-01T00:00:00+00:00";
const LEGACY_COMMIT: &str = "3f4a65f000000000000000000000000000000000";
const MAIN_OLD: &str = "gh:keelstackhq/hull//template-rust";
const MAIN_NEW: &str = "gh:keelstackhq/keelstack//hull/template-rust";
const CHAN_OLD: &str = "gh:keelstackhq/bosun//channel";
const CHAN_NEW: &str = "gh:keelstackhq/keelstack//bosun/channel";

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn fixture(rel: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

/// A directory holding `keelstack.git`, a stand-in for the monorepo: commit 1 has hull/template-rust
/// and bosun/channel at v1, commit 2 (tagged v2.0.0) has v2 plus `<tool>/legacy-tags.txt` mapping
/// `v0.13.0` (hull) and `v0.21.0` (bosun) to commit 1.
fn monorepo() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("keelstack.git");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, DATE, &["init", "-q", "-b", "main"]);
    copy_tree(&fixture("template/v1"), &repo.join("hull/template-rust"));
    copy_tree(&fixture("channel/v1"), &repo.join("bosun/channel"));
    stage_all(&repo, DATE);
    git(&repo, DATE, &["commit", "-q", "-m", "v1"]);
    let c1 = git_stdout(&repo, &["rev-parse", "HEAD"]);
    std::fs::remove_dir_all(repo.join("hull")).unwrap();
    std::fs::remove_dir_all(repo.join("bosun")).unwrap();
    copy_tree(&fixture("template/v2"), &repo.join("hull/template-rust"));
    copy_tree(&fixture("channel/v2"), &repo.join("bosun/channel"));
    std::fs::write(
        repo.join("hull/legacy-tags.txt"),
        format!("# legacy tags\nv0.12.0 {}\nv0.13.0 {c1}\n", "0".repeat(40)),
    )
    .unwrap();
    std::fs::write(
        repo.join("bosun/legacy-tags.txt"),
        format!("v0.21.0 {c1}\n"),
    )
    .unwrap();
    stage_all(&repo, DATE);
    git(&repo, DATE, &["commit", "-q", "-m", "v2"]);
    git(&repo, DATE, &["tag", "v2.0.0"]);
    root
}

fn splice(mono: &TempDir) -> assert_cmd::Command {
    let mut cmd = splice_cmd();
    cmd.env("GIT_CONFIG_COUNT", "1")
        .env(
            "GIT_CONFIG_KEY_0",
            format!("url.{}/.insteadOf", mono.path().display()),
        )
        .env("GIT_CONFIG_VALUE_0", "https://github.com/keelstackhq/")
        // Own cache per test: parallel tests share one URL key and race on the mirror clone.
        .env("SPLICE_CACHE_DIR", mono.path().join("cache"));
    cmd
}

fn lock_text(repo: &Path, rel: &str) -> String {
    std::fs::read_to_string(repo.join(rel)).unwrap()
}

/// Rewrite the `[template]` source, version and commit of a lock file to a legacy source.
fn make_legacy(repo: &Path, rel: &str, source: &str, version: &str) {
    let text = lock_text(repo, rel);
    let mut in_template = false;
    let mut out = String::new();
    for line in text.lines() {
        let l = if line.starts_with('[') {
            in_template = line == "[template]";
            line.to_string()
        } else if in_template && line.starts_with("source = ") {
            format!("source = \"{source}\"")
        } else if in_template && line.starts_with("version = ") {
            format!("version = \"{version}\"")
        } else if in_template && line.starts_with("commit = ") {
            format!("commit = \"{LEGACY_COMMIT}\"")
        } else {
            line.to_string()
        };
        out.push_str(&l);
        out.push('\n');
    }
    std::fs::write(repo.join(rel), out).unwrap();
    commit_all(repo);
}

fn subject(repo: &Path) -> String {
    git_stdout(repo, &["log", "-1", "--format=%s"])
}

#[test]
fn legacy_main_source_repoints_and_updates_in_one_run() {
    let mono = monorepo();
    let tpl = template_repo(&["v1"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let r = repo.path();
    make_legacy(r, ".splice/lock.toml", MAIN_OLD, "0.13.0");
    let before = git_stdout(r, &["rev-list", "--count", "HEAD"]);

    splice(&mono)
        .arg("-C")
        .arg(r)
        .args(["--yes", "--json", "update", "--all"])
        .assert()
        .success();

    let lock = lock_text(r, ".splice/lock.toml");
    assert!(lock.contains(&format!("source = \"{MAIN_NEW}\"")), "{lock}");
    assert!(lock.contains("version = \"2.0.0\""), "{lock}");
    assert!(!lock.contains(LEGACY_COMMIT), "{lock}");
    assert_eq!(
        subject(r),
        format!("chore(splice): repoint {MAIN_OLD} to the monorepo")
    );
    let after = git_stdout(r, &["rev-list", "--count", "HEAD"]);
    assert_eq!(
        after.parse::<u32>().unwrap(),
        before.parse::<u32>().unwrap() + 1
    );
}

#[test]
fn legacy_channel_source_repoints() {
    let mono = monorepo();
    let tpl = template_repo(&["v1"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let r = repo.path();
    let ch = channel_repo(&["v1"]);
    chan_commit(
        r,
        &[
            "channel",
            "add",
            &format!("{}//channel", ch.path().display()),
            "--ref",
            "v1.0.0",
            "--piece",
            "claude",
        ],
    );
    make_legacy(r, ".splice/channels/rig.toml", CHAN_OLD, "0.21.0");

    splice(&mono)
        .arg("-C")
        .arg(r)
        .args(["--yes", "--json", "update", "--channel", "rig"])
        .assert()
        .success();

    let lock = lock_text(r, ".splice/channels/rig.toml");
    assert!(lock.contains(&format!("source = \"{CHAN_NEW}\"")), "{lock}");
    assert!(lock.contains("version = \"2.0.0\""), "{lock}");
    assert_eq!(
        subject(r),
        format!("chore(splice): repoint {CHAN_OLD} to the monorepo")
    );
}

#[test]
fn unknown_legacy_version_fails_and_writes_nothing() {
    let mono = monorepo();
    let tpl = template_repo(&["v1"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let r = repo.path();
    make_legacy(r, ".splice/lock.toml", MAIN_OLD, "0.99.0");
    let head = git_stdout(r, &["rev-parse", "HEAD"]);
    let lock = lock_text(r, ".splice/lock.toml");

    let out = splice(&mono)
        .arg("-C")
        .arg(r)
        .args(["--yes", "update", "--all"])
        .assert()
        .code(1);
    let o = out.get_output();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(text.contains("hull/legacy-tags.txt"), "{text}");
    assert_eq!(git_stdout(r, &["rev-parse", "HEAD"]), head);
    assert_eq!(git_stdout(r, &["status", "--porcelain"]), "");
    assert_eq!(lock_text(r, ".splice/lock.toml"), lock);
}
