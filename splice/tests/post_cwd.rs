//! A channel's `[[post]]` commands run in the channel's `only` root and are skipped, never
//! failed, when the manifest they need is missing there (UC-S2-1). Offline fixtures.

mod common;

use common::*;
use std::path::Path;
use tempfile::TempDir;

const DATE: &str = "2026-01-01T00:00:00+00:00";

/// A git repo with `channel/` holding one base piece `p` whose file is `x<n>` and whose post
/// command is `run` (when add + update); tagged v1.0.0 and, with `v2`, v2.0.0 (new file).
fn channel_with_post(run: &str, v2: bool) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), DATE, &["init", "-q", "-b", "main"]);
    let versions: &[(&str, &str)] = if v2 {
        &[("one", "v1.0.0"), ("two", "v2.0.0")]
    } else {
        &[("one", "v1.0.0")]
    };
    for (i, (content, tag)) in versions.iter().enumerate() {
        let tpl = inline_template(&[(
            "p",
            &format!(
                "id = \"p\"\ndescription = \"p\"\n\n[files]\nowned = [\"rig.txt\"]\n\n\
                 [[post]]\nrun = \"{run}\"\nwhen = [\"add\", \"update\"]\n"
            ),
            &[("rig.txt", content)],
        )]);
        let sub = repo.path().join("channel");
        if sub.exists() {
            std::fs::remove_dir_all(&sub).unwrap();
        }
        copy(tpl.path(), &sub);
        stage_all(repo.path(), DATE);
        git(repo.path(), DATE, &["commit", "-q", "-m", &format!("v{i}")]);
        git(repo.path(), DATE, &["tag", tag]);
    }
    repo
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn run(repo: &Path, cache: &Path, args: &[&str]) -> (String, assert_cmd::assert::Assert) {
    let a = splice_cmd()
        .env("SPLICE_CACHE_DIR", cache)
        .arg("-C")
        .arg(repo)
        .args(["--yes", "--json"])
        .args(args)
        .assert();
    (String::new(), a)
}

fn stderr(a: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&a.get_output().stderr).into_owned()
}

fn add_channel(repo: &Path, cache: &Path, src: &Path, only: &str) {
    let spec = format!("{}//channel", src.display());
    let (_, a) = run(
        repo,
        cache,
        &["channel", "add", &spec, "--ref", "v1.0.0", "--only", only],
    );
    a.success();
    commit_all(repo);
}

#[test]
fn update_without_root_package_json_skips_pnpm_and_exits_0() {
    let ch = channel_with_post("pnpm install", true);
    let repo = bare_repo();
    let cache = tempfile::tempdir().unwrap();
    // The `only` root exists but has no package.json; the repo root has none either.
    std::fs::create_dir_all(repo.path().join("apps/site")).unwrap();
    std::fs::write(repo.path().join("apps/site/.keep"), "").unwrap();
    commit_all(repo.path());
    add_channel(repo.path(), cache.path(), ch.path(), "apps/site/**");

    let (_, a) = run(repo.path(), cache.path(), &["update", "--channel", "t"]);
    let a = a.success();
    let err = stderr(&a);
    assert!(
        err.contains("skipped") && err.contains("apps/site"),
        "{err}"
    );
}

#[test]
fn post_runs_in_the_only_root() {
    let ch = channel_with_post("touch ran.txt", false);
    let repo = bare_repo();
    let cache = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("apps/site")).unwrap();
    std::fs::write(repo.path().join("apps/site/.keep"), "").unwrap();
    commit_all(repo.path());
    add_channel(repo.path(), cache.path(), ch.path(), "apps/site/**");
    assert!(repo.path().join("apps/site/ran.txt").exists());
    assert!(!repo.path().join("ran.txt").exists());
}

#[test]
fn missing_only_root_skips_the_command() {
    let ch = channel_with_post("touch ran.txt", false);
    let repo = bare_repo();
    let cache = tempfile::tempdir().unwrap();
    let spec = format!("{}//channel", ch.path().display());
    let (_, a) = run(
        repo.path(),
        cache.path(),
        &[
            "channel",
            "add",
            &spec,
            "--ref",
            "v1.0.0",
            "--only",
            "nowhere/**",
        ],
    );
    let a = a.success();
    assert!(stderr(&a).contains("skipped"), "{}", stderr(&a));
    assert!(!repo.path().join("ran.txt").exists());
}

#[test]
fn pnpm_runs_where_package_json_exists() {
    let ch = channel_with_post("test -f package.json && touch ran.txt", false);
    let repo = bare_repo();
    let cache = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(repo.path().join("apps/site")).unwrap();
    std::fs::write(repo.path().join("apps/site/package.json"), "{}\n").unwrap();
    commit_all(repo.path());
    add_channel(repo.path(), cache.path(), ch.path(), "apps/site/**");
    assert!(repo.path().join("apps/site/ran.txt").exists());
}
