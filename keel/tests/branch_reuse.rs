//! K4 (#119): `--branch B` after B's PR merged starts fresh from the base and pushes plainly
//! (never forced). Real temp git repos with a bare origin; no network.

use keel::exec::SystemRunner;
use keel::tools::git::branch_start;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str) {
    std::fs::write(dir.join(file), file).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-m", file]);
}

struct Repo {
    _t: tempfile::TempDir,
    work: PathBuf,
}

fn repo() -> Repo {
    let t = tempfile::tempdir().unwrap();
    let bare = t.path().join("origin.git");
    let work = t.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(t.path(), &["init", "--bare", "-b", "main", "origin.git"]);
    git(&work, &["init", "-b", "main"]);
    git(
        &work,
        &["remote", "add", "origin", &bare.display().to_string()],
    );
    commit(&work, "a");
    git(&work, &["push", "-u", "origin", "main"]);
    Repo { _t: t, work }
}

/// B pushed with one commit, then merged into main with a merge commit.
fn merge_b(r: &Repo) {
    git(&r.work, &["checkout", "-b", "B"]);
    commit(&r.work, "b");
    git(&r.work, &["push", "-u", "origin", "B"]);
    git(&r.work, &["checkout", "main"]);
    git(&r.work, &["merge", "--no-ff", "-m", "merge B", "B"]);
    git(&r.work, &["push", "origin", "main"]);
}

fn start(r: &Repo) -> String {
    branch_start(&SystemRunner, &r.work, "B", "origin/main")
}

fn rev(r: &Repo, x: &str) -> String {
    git(&r.work, &["rev-parse", x])
}

/// Detached at `start`, one new commit, plain push to B (what migrate does).
fn push_from(r: &Repo, start: &str) {
    git(&r.work, &["checkout", "--detach", start]);
    commit(&r.work, "again");
    git(&r.work, &["push", "origin", "HEAD:refs/heads/B"]);
}

#[test]
fn uc_k4_1_second_run_after_merge_with_remote_branch_gone_pushes_plainly() {
    let r = repo();
    merge_b(&r);
    git(&r.work, &["push", "origin", "--delete", "B"]);
    git(&r.work, &["fetch", "--prune", "origin"]);
    let s = start(&r);
    assert_eq!(rev(&r, &s), rev(&r, "origin/main"), "starts from the base");
    push_from(&r, &s);
    assert_eq!(
        git(&r.work, &["ls-remote", "origin", "B"])
            .split('\t')
            .next()
            .unwrap(),
        rev(&r, "HEAD")
    );
}

#[test]
fn remote_branch_merged_into_base_starts_from_base_and_fast_forwards() {
    let r = repo();
    merge_b(&r);
    git(&r.work, &["branch", "-D", "B"]);
    let s = start(&r);
    assert_eq!(rev(&r, &s), rev(&r, "origin/main"));
    push_from(&r, &s);
    assert_eq!(
        git(&r.work, &["ls-remote", "origin", "B"])
            .split('\t')
            .next()
            .unwrap(),
        rev(&r, "HEAD")
    );
}

#[test]
fn an_unmerged_local_branch_never_pushed_is_kept() {
    let r = repo();
    git(&r.work, &["checkout", "-b", "B"]);
    commit(&r.work, "wip");
    git(&r.work, &["checkout", "main"]);
    assert_eq!(start(&r), "B");
}

#[test]
fn an_unmerged_remote_branch_is_resumed() {
    let r = repo();
    git(&r.work, &["checkout", "-b", "B"]);
    commit(&r.work, "wip");
    git(&r.work, &["push", "-u", "origin", "B"]);
    git(&r.work, &["checkout", "main"]);
    git(&r.work, &["branch", "-D", "B"]);
    assert_eq!(start(&r), "origin/B");
}

#[test]
fn source_never_forces() {
    {
        let f = "../src/tools/git.rs";
        let src =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(f))
                .unwrap();
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains("--force") && !code.contains("\"-f\""), "{f}");
    }
}
