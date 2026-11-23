//! test-integrity skips template sources (spec §7): a file under a `template*/pieces/<piece>/files/`
//! or `channel/pieces/<piece>/files/` tree is render input, not a test of this repo, so moving it
//! to `.jinja` or dropping a test there is no finding.

mod common;

use common::{bosun, one_object};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

const TWO_TESTS: &str =
    "#[test]\nfn a() {\n    assert!(true);\n}\n\n#[test]\nfn b() {\n    let _ = 1;\n}\n";

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

/// Base has `.bosun/` (not a bootstrap PR) and the given test file; the branch moves it.
fn run(path: &str, moved: &str) -> (i32, Value) {
    run_with(path, moved, true)
}

fn run_with(path: &str, moved: &str, marker: bool) -> (i32, Value) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    put(&root, ".bosun/bosun.toml", "");
    put(&root, path, TWO_TESTS);
    if marker {
        // The marker sits at the template folder's root: the dir holding `pieces/`.
        let dir = match path.split_once("/pieces/") {
            Some((dir, _)) => dir,
            None => path.split('/').next().unwrap(),
        };
        put(&root, &format!("{dir}/splice.toml"), "");
    }
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    git(&root, &["checkout", "-q", "-b", "work"]);
    git(&root, &["mv", path, moved]);
    git(&root, &["commit", "-q", "-m", "move"]);
    let out = bosun()
        .env_remove("GITHUB_BASE_REF")
        .env_remove("BOSUN_PR_BODY")
        .env_remove("BOSUN_INTEGRITY_ALLOW")
        .arg("-C")
        .arg(&root)
        .args(["test-integrity", "--base", "main"])
        .output()
        .unwrap();
    (out.status.code().unwrap(), one_object(&out.stdout))
}

#[test]
fn a_template_test_moved_to_jinja_is_no_finding() {
    let (code, v) = run(
        "template-rust/pieces/core/files/tests/t.rs",
        "template-rust/pieces/core/files/tests/t.rs.jinja",
    );
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_channel_piece_test_moved_to_jinja_is_no_finding() {
    let (code, v) = run(
        "channel/pieces/rust/files/tests/t.rs",
        "channel/pieces/rust/files/tests/t.rs.jinja",
    );
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_monorepo_template_test_moved_to_jinja_is_no_finding() {
    let (code, v) = run(
        "hull/template-rust/pieces/core/files/tests/t.rs",
        "hull/template-rust/pieces/core/files/tests/t.rs.jinja",
    );
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_monorepo_channel_piece_test_moved_to_jinja_is_no_finding() {
    let (code, v) = run(
        "bosun/channel/pieces/rust/files/tests/t.rs",
        "bosun/channel/pieces/rust/files/tests/t.rs.jinja",
    );
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_monorepo_marker_added_by_the_pr_does_not_exempt() {
    let (code, v) = marker_added_by_the_pr(
        "hull/template/pieces/core/files/tests/t.rs",
        "hull/template/splice.toml",
    );
    assert_eq!(code, 1, "{v}");
}

#[test]
fn a_real_test_moved_to_jinja_is_still_refused() {
    let (code, v) = run("tests/t.rs", "tests/t.rs.jinja");
    assert_eq!(code, 1, "{v}");
}

#[test]
fn a_look_alike_path_without_a_template_marker_is_still_refused() {
    let (code, v) = run_with(
        "template-rust/pieces/core/files/tests/t.rs",
        "template-rust/pieces/core/files/tests/t.rs.jinja",
        false,
    );
    assert_eq!(code, 1, "{v}");
}

/// Base has `.bosun/` and a test under `path` but no marker; the PR adds the marker and deletes
/// the test. The marker is read at the base ref, not from the PR, so this stays a finding.
fn marker_added_by_the_pr(path: &str, marker: &str) -> (i32, Value) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    put(&root, ".bosun/bosun.toml", "");
    put(&root, path, TWO_TESTS);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    git(&root, &["checkout", "-q", "-b", "work"]);
    put(&root, marker, "");
    git(&root, &["rm", "-q", path]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "add marker, drop test"]);
    let out = bosun()
        .env_remove("GITHUB_BASE_REF")
        .env_remove("BOSUN_PR_BODY")
        .env_remove("BOSUN_INTEGRITY_ALLOW")
        .arg("-C")
        .arg(&root)
        .args(["test-integrity", "--base", "main"])
        .output()
        .unwrap();
    (out.status.code().unwrap(), one_object(&out.stdout))
}

#[test]
fn a_marker_added_by_the_pr_does_not_exempt_a_template_path() {
    let (code, v) = marker_added_by_the_pr(
        "template-rust/pieces/core/files/tests/t.rs",
        "template-rust/splice.toml",
    );
    assert_eq!(code, 1, "{v}");
}

#[test]
fn a_channel_marker_added_by_the_pr_does_not_exempt_a_channel_path() {
    let (code, v) = marker_added_by_the_pr(
        "channel/pieces/rust/files/tests/t.rs",
        "channel/splice.toml",
    );
    assert_eq!(code, 1, "{v}");
}

/// The marker is read at the base tip, the ref the PR merges into, not at the merge base: when
/// base moved on after the branch point (here: it gained the marker, and its own commit sits
/// between the merge base and the tip), the branch's move to `.jinja` is exempt; when base never
/// had a marker it stays a finding, whatever the branch adds.
#[test]
fn the_marker_is_read_at_the_base_tip_when_it_differs_from_the_merge_base() {
    let path = "template-rust/pieces/core/files/tests/t.rs";
    let moved = "template-rust/pieces/core/files/tests/t.rs.jinja";
    for (marker_on_main, expect) in [(true, 0), (false, 1)] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "t"]);
        git(&root, &["config", "user.email", "t@example.com"]);
        put(&root, ".bosun/bosun.toml", "");
        put(&root, path, TWO_TESTS);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "base"]);
        git(&root, &["checkout", "-q", "-b", "work"]);
        git(&root, &["mv", path, moved]);
        git(&root, &["commit", "-q", "-m", "move"]);
        // Base moves on after the branch point.
        git(&root, &["checkout", "-q", "main"]);
        put(&root, "NOTES.md", "later\n");
        if marker_on_main {
            put(&root, "template-rust/splice.toml", "");
        }
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "main moves on"]);
        git(&root, &["checkout", "-q", "work"]);
        let mb = Command::new("git")
            .current_dir(&root)
            .args(["merge-base", "main", "HEAD"])
            .output()
            .unwrap();
        let tip = Command::new("git")
            .current_dir(&root)
            .args(["rev-parse", "main"])
            .output()
            .unwrap();
        assert_ne!(
            mb.stdout, tip.stdout,
            "merge base must differ from the base tip"
        );
        let out = bosun()
            .env_remove("GITHUB_BASE_REF")
            .env_remove("BOSUN_PR_BODY")
            .env_remove("BOSUN_INTEGRITY_ALLOW")
            .arg("-C")
            .arg(&root)
            .args(["test-integrity", "--base", "main"])
            .output()
            .unwrap();
        let v = one_object(&out.stdout);
        assert_eq!(
            out.status.code(),
            Some(expect),
            "marker on main {marker_on_main}: {v}"
        );
    }
}
