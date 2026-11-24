//! UC-WB-2: one big PR closes many issues across folders (modelled on the keelstack big PR:
//! ~45 `Closes` lines, a handful of them bosun tasks, the rest plain issues). test-integrity
//! accepts it when the tasks' touch together covers every file, and each task's `allow`
//! reaches only the files its own touch covers.
mod common;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("git");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

const TEST: &str = "#[test]\nfn t() {\n    assert!(true);\n}\n";

/// A monorepo base with tests in bosun/ and hoist/, and a head that edits files in five folders,
/// deletes `bosun/tests/old_bundle.rs` and `deleted` (when given).
fn big_repo(deleted: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap().join("repo");
    for d in ["bosun/tests", "hoist/tests", "hull", "keel/src", ".bosun"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    fs::write(root.join(".bosun/bosun.toml"), "").unwrap();
    for f in ["bosun/tests/old_bundle.rs", "hoist/tests/cli_r2.rs"] {
        fs::write(root.join(f), TEST).unwrap();
    }
    fs::write(root.join("hull/guard.mjs"), "x\n").unwrap();
    fs::write(root.join("keel/src/main.rs"), "fn main() {}\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "base"]);
    git(&root, &["checkout", "-q", "-b", "claude/big-pr"]);
    fs::remove_file(root.join("bosun/tests/old_bundle.rs")).unwrap();
    for f in deleted {
        fs::remove_file(root.join(f)).unwrap();
    }
    fs::write(root.join("hull/guard.mjs"), "y\n").unwrap();
    fs::write(root.join("keel/src/main.rs"), "fn main() { }\n").unwrap();
    git(&root, &["commit", "-qam", "big"]);
    (tmp, root)
}

fn task(id: &str, touch: &str, allow: &str) -> String {
    let (item, t) = id.split_once('/').unwrap();
    format!(
        "<!-- bosun:plan {id} -->\n<!-- bosun:task {{\"item\":\"{item}\",\"task\":\"{t}\",\"touch\":{touch},\"allow\":{allow}}} -->"
    )
}

/// The big PR #100: `Closes` for 45 issues (plain issues #6..#64 plus the plan's task issues
/// #201..#205), as GitHub lists its files.
fn big_pr(fake: &FakeGithub, files: &[(&str, &str)], in_touch: &str) {
    let plain = [
        6, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 31, 32,
        33, 34, 36, 37, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 64,
    ];
    let mut body = String::from("One big PR.\n\n");
    for n in plain.iter().chain(&[201, 202, 203, 204, 205]) {
        body.push_str(&format!("Closes #{n}\n"));
    }
    fake.pull(100, "one big PR", "claude/big-pr", &body);
    for (f, status) in files {
        fake.pull_file(100, f, status, 1, 1, None);
    }
    for n in plain {
        // Plain issues: no task metadata, they lend nothing and need no touch.
        fake.issue(n, &format!("issue {n}"), &[], "words");
    }
    fake.issue(
        201,
        "WB/T1",
        &[],
        &task(
            "WB/T1",
            r#"["bosun/src/**","bosun/tests/cli_big_pr_*.rs","bosun/tests/old_bundle.rs"]"#,
            r#"["delete-test"]"#,
        ),
    );
    fake.issue(202, "WG/T1", &[], &task("WG/T1", r#"["hull/**"]"#, "[]"));
    fake.issue(203, "WH/T1", &[], &task("WH/T1", r#"["keel/**"]"#, "[]"));
    fake.issue(
        204,
        "WH/T2",
        &[],
        &task("WH/T2", r#"["hoist/tests/**"]"#, "[]"),
    );
    // IN/T2: the integrator's bookkeeping task touches everything and allows nothing.
    fake.issue(205, "IN/T2", &[], &task("IN/T2", in_touch, "[]"));
}

fn check(fake: &FakeGithub, root: &Path) -> (i32, Value) {
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env_remove("BOSUN_PR_BODY")
        .env_remove("BOSUN_INTEGRITY_ALLOW")
        .arg("-C")
        .arg(root)
        .args(["test-integrity", "--base", "main", "--pr", "100"])
        .output()
        .unwrap();
    (out.status.code().unwrap(), one_object(&out.stdout))
}

const FILES: [(&str, &str); 3] = [
    ("bosun/tests/old_bundle.rs", "removed"),
    ("hull/guard.mjs", "modified"),
    ("keel/src/main.rs", "modified"),
];

#[test]
fn the_big_pr_passes_with_each_allow_on_its_own_touch() {
    let (_tmp, root) = big_repo(&[]);
    let fake = FakeGithub::start();
    big_pr(&fake, &FILES, r#"["**"]"#);
    let (code, v) = check(&fake, &root);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["tasks"], json!([201, 202, 203, 204, 205]));
    assert_eq!(v["data"]["allow"], json!(["delete-test"]));
    assert_eq!(
        v["data"]["grants"],
        json!([{"task": 201, "allow": ["delete-test"]}])
    );
}

#[test]
fn an_allow_does_not_reach_another_tasks_files() {
    // hoist/tests/cli_r2.rs is covered by WH/T2 and IN/T2, neither allows delete-test; WB/T1's
    // allow stops at its own touch.
    let (_tmp, root) = big_repo(&["hoist/tests/cli_r2.rs"]);
    let fake = FakeGithub::start();
    let mut files = FILES.to_vec();
    files.push(("hoist/tests/cli_r2.rs", "removed"));
    big_pr(&fake, &files, r#"["**"]"#);
    let (code, v) = check(&fake, &root);
    assert_eq!(code, 1, "{v}");
    let refused: Vec<&Value> = (v["data"]["findings"].as_array().unwrap().iter())
        .filter(|f| f["allowed"] == false)
        .collect();
    assert_eq!(refused.len(), 1, "{v}");
    assert_eq!(refused[0]["file"], "hoist/tests/cli_r2.rs");
    // The flat list the CI gate passes never carries a kind with a refused finding.
    assert_eq!(v["data"]["allow"], json!([]));
}

#[test]
fn a_file_outside_every_tasks_touch_lends_nothing() {
    let (_tmp, root) = big_repo(&[]);
    let fake = FakeGithub::start();
    // Without IN/T2's `**`, site/ is outside every task's touch.
    big_pr(&fake, &FILES, r#"["decisions/**"]"#);
    fake.pull_file(100, "site/index.html", "modified", 1, 1, None);
    let (code, v) = check(&fake, &root);
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["data"]["allow"], json!([]));
    let why = v["data"]["allow_refused"].as_str().unwrap();
    assert!(why.contains("site/index.html"), "{why}");
}
