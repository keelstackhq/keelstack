//! `splice set [--channel NAME] K=V...` (UC-S3-1): change a locked template var and re-render
//! only the owned files it touches, at the locked commit. Offline: local git fixtures.

mod common;

use common::*;
use std::path::Path;
use tempfile::TempDir;

const DATE: &str = "2026-01-01T00:00:00+00:00";

const PIECE: &str = "id = \"core\"\ndescription = \"core\"\nmultiple = false\n\n[files]\n\
owned = [\"risk.txt\", \"verdict.txt\", \"plain.txt\"]\nshared = [\"shared.txt\"]\nseed = [\"seed.txt\"]\n";

/// A channel repo (`channel/` subdir, tag v1.0.0) with var `bosun_app`: two owned files, one
/// shared and one seed reference it; `plain.txt` does not.
fn channel_source() -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    let r = repo.path();
    git(r, DATE, &["init", "-q", "-b", "main"]);
    let c = r.join("channel");
    let files = c.join("pieces/core/files");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(
        c.join("splice.toml"),
        "format = 1\nname = \"bosun\"\nmin_splice = \"0.2.0\"\nbase = \"core\"\n\n[vars]\n\
         bosun_app = { prompt = \"bosun app id\", default = \"0\", pattern = \"^[0-9]+$\" }\n",
    )
    .unwrap();
    std::fs::write(c.join("pieces/core/piece.toml"), PIECE).unwrap();
    for f in ["risk.txt", "verdict.txt", "shared.txt", "seed.txt"] {
        std::fs::write(
            files.join(format!("{f}.jinja")),
            format!("{f} app={{{{ bosun_app }}}}\n"),
        )
        .unwrap();
    }
    std::fs::write(files.join("plain.txt"), "plain\n").unwrap();
    stage_all(r, DATE);
    git(r, DATE, &["commit", "-q", "-m", "v1"]);
    git(r, DATE, &["tag", "v1.0.0"]);
    repo
}

/// A clean repo with channel `bosun` added at `bosun_app=0`.
fn setup() -> (TempDir, TempDir, TempDir) {
    let src = channel_source();
    let cache = tempfile::tempdir().unwrap();
    let repo = bare_repo();
    let out = splice(cache.path())
        .arg("-C")
        .arg(repo.path())
        .args(["--yes", "--json", "channel", "add"])
        .arg(format!("{}//channel", src.path().display()))
        .args(["--as", "bosun", "--ref", "v1.0.0"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    commit_all(repo.path());
    (repo, src, cache)
}

fn splice(cache: &Path) -> assert_cmd::Command {
    let mut cmd = splice_cmd();
    // Own cache per test: parallel tests must not share one mirror clone.
    cmd.env("SPLICE_CACHE_DIR", cache);
    cmd
}

fn set(repo: &Path, cache: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    splice(cache)
        .arg("-C")
        .arg(repo)
        .args(["--yes", "--json", "set"])
        .args(args)
        .assert()
}

fn read(repo: &Path, rel: &str) -> String {
    std::fs::read_to_string(repo.join(rel)).unwrap()
}

#[test]
fn set_rewrites_only_files_that_reference_the_var() {
    let (repo, _src, cache) = setup();
    let r = repo.path();
    let lock_before = read(r, ".splice/channels/bosun.toml");

    let out = set(r, cache.path(), &["--channel", "bosun", "bosun_app=1"]).success();
    let v: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(v["command"], "set");
    assert_eq!(v["ok"], true);
    let mut paths: Vec<&str> = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    paths.sort();
    assert_eq!(paths, ["risk.txt", "verdict.txt"]);

    assert_eq!(read(r, "risk.txt"), "risk.txt app=1\n");
    assert_eq!(read(r, "verdict.txt"), "verdict.txt app=1\n");
    // Shared and seed files and unrelated owned files stay as they were.
    assert_eq!(read(r, "shared.txt"), "shared.txt app=0\n");
    assert_eq!(read(r, "seed.txt"), "seed.txt app=0\n");
    assert_eq!(read(r, "plain.txt"), "plain\n");
    let lock = read(r, ".splice/channels/bosun.toml");
    assert_ne!(lock, lock_before);
    assert!(lock.contains("bosun_app = \"1\""), "{lock}");

    splice(cache.path())
        .arg("-C")
        .arg(r)
        .args(["--json", "check", "--offline"])
        .assert()
        .success();
}

#[test]
fn set_main_template_var() {
    let tpl = template_repo(&["v1"]);
    let cache = tempfile::tempdir().unwrap();
    let repo = new_repo_at(&tpl, "v1.0.0");
    let r = repo.path();
    set(r, cache.path(), &["scope=Bad Scope"]).code(2);
    set(r, cache.path(), &["nope=1"]).code(2);
    set(r, cache.path(), &["scope=zeta"]).success();
    assert!(read(r, ".splice/lock.toml").contains("scope = \"zeta\""));
    splice(cache.path())
        .arg("-C")
        .arg(r)
        .args(["--json", "check", "--offline"])
        .assert()
        .success();
}

#[test]
fn invalid_or_unknown_var_is_a_usage_error_and_writes_nothing() {
    let (repo, _src, cache) = setup();
    let r = repo.path();
    for arg in ["bosun_app=x1", "nope=1", "bosun_app"] {
        set(r, cache.path(), &["--channel", "bosun", arg]).code(2);
    }
    set(r, cache.path(), &["--channel", "ghost", "bosun_app=1"]).code(2);
    assert_eq!(git_stdout(r, &["status", "--porcelain"]), "");
}

#[test]
fn drifted_owned_file_is_refused_unless_forced() {
    let (repo, _src, cache) = setup();
    let r = repo.path();
    std::fs::write(r.join("risk.txt"), "my edit\n").unwrap();
    commit_all(r);
    let lock = read(r, ".splice/channels/bosun.toml");

    set(r, cache.path(), &["--channel", "bosun", "bosun_app=1"]).code(1);
    assert_eq!(read(r, "risk.txt"), "my edit\n");
    assert_eq!(read(r, "verdict.txt"), "verdict.txt app=0\n");
    assert_eq!(read(r, ".splice/channels/bosun.toml"), lock);

    set(
        r,
        cache.path(),
        &["--channel", "bosun", "--force", "bosun_app=1"],
    )
    .success();
    assert_eq!(read(r, "risk.txt"), "risk.txt app=1\n");
}

#[test]
fn dirty_tree_is_refused_unless_allowed() {
    let (repo, _src, cache) = setup();
    let r = repo.path();
    std::fs::write(r.join("unrelated.txt"), "x\n").unwrap();
    set(r, cache.path(), &["--channel", "bosun", "bosun_app=1"]).code(2);
    assert_eq!(read(r, "risk.txt"), "risk.txt app=0\n");
    set(
        r,
        cache.path(),
        &["--channel", "bosun", "--allow-dirty", "bosun_app=1"],
    )
    .success();
    assert_eq!(read(r, "risk.txt"), "risk.txt app=1\n");
}
