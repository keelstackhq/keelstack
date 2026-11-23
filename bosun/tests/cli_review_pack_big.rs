//! One big PR that closes several tasks: the pack lists every task and every chunk, drift is
//! computed over the tasks that have a `size:`, and `not_shown` names the exact paths.

mod common;

use common::fake::FakeGithub;
use common::run;
use serde_json::{Value, json};

const T0: &str = "2026-10-22T10:00:00Z";

fn task(fake: &FakeGithub, n: u64, size: &str) {
    let body = format!(
        r#"Do it.<!-- bosun:task {{"item":"M1","task":"T{n}","touch":["src/t{n}/**"]{size}}} -->"#
    );
    fake.issue(n, &format!("task {n}"), &["bosun:state/review"], &body);
}

/// 10 source files of 1000 changed lines each (a ~30 KB patch: one chunk per file), a test
/// file, a lockfile and a binary file.
fn big_pr() -> FakeGithub {
    let fake = FakeGithub::start();
    task(&fake, 7, r#","size":3000"#);
    task(&fake, 8, r#","size":1000"#);
    task(&fake, 9, "");
    fake.issue(5, "plain issue", &["bug"], "not a task");
    fake.pull_as_issue(42, "the big pr");
    fake.pull(
        42,
        "the big pr",
        "claude/big",
        "Closes #7\nCloses #8\nCloses #9\nSee #5",
    );
    let patch = "+x".repeat(15_000);
    for i in 0..10 {
        fake.pull_file(
            42,
            &format!("src/t{}/f{i}.rs", 7 + i % 3),
            "added",
            1000,
            0,
            Some(&patch),
        );
    }
    fake.pull_file(42, "tests/big.rs", "added", 4000, 0, Some("@@ t"));
    fake.pull_file(42, "Cargo.lock", "modified", 900, 0, Some("@@ l"));
    fake.pull_file(42, "assets/logo.png", "added", 0, 0, None);
    fake
}

fn pack(fake: &FakeGithub) -> Value {
    let (code, v) = run(fake, "s", T0, &["review-pack", "42"]);
    assert_eq!(code, 0, "{v}");
    v["data"].clone()
}

#[test]
fn every_closing_task_is_listed_and_the_first_stays_in_task() {
    let d = pack(&big_pr());
    let tasks = d["tasks"].as_array().unwrap();
    let nums: Vec<u64> = tasks.iter().map(|t| t["issue"].as_u64().unwrap()).collect();
    assert_eq!(nums, [7, 8, 9], "the plain issue #5 is not a task: {d}");
    assert_eq!(tasks[0]["title"], "task 7");
    assert_eq!(tasks[0]["touch"], json!(["src/t7/**"]));
    assert_eq!(tasks[0]["size"], 3000);
    assert_eq!(tasks[2]["size"], Value::Null);
    assert_eq!(d["task"]["issue"], 7);
    assert_eq!(d["unsized_tasks"], json!([9]));
}

#[test]
fn drift_is_over_the_sized_tasks_and_the_unsized_one_does_not_hide_it() {
    let d = pack(&big_pr());
    // 10 source files x 1000 lines; tests and lockfiles do not count. Planned 3000 + 1000.
    assert_eq!(
        d["drift"],
        json!({"planned": 4000, "actual": 10_000}),
        "{d}"
    );
    assert_eq!(
        d["note"],
        "re-plan? PR #42 is 10000 changed lines, planned 4000"
    );
}

#[test]
fn drift_is_null_only_when_no_task_is_sized() {
    let fake = FakeGithub::start();
    task(&fake, 7, "");
    task(&fake, 8, "");
    fake.pull_as_issue(42, "p");
    fake.pull(42, "p", "claude/x", "Closes #7, closes #8");
    fake.pull_file(42, "src/t7/a.rs", "modified", 9000, 0, Some("@@ a"));
    let d = pack(&fake);
    assert_eq!(d["drift"], Value::Null);
    assert_eq!(d["unsized_tasks"], json!([7, 8]));
    assert_eq!(d["tasks"].as_array().unwrap().len(), 2);
}

#[test]
fn every_chunk_is_listed_and_not_shown_names_the_exact_paths() {
    let d = pack(&big_pr());
    let chunks = d["chunks"].as_array().unwrap();
    let listed: Vec<&str> = chunks
        .iter()
        .flat_map(|c| c.as_array().unwrap())
        .map(|f| f.as_str().unwrap())
        .collect();
    let mut names: Vec<&str> = d["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["filename"].as_str().unwrap())
        .collect();
    assert_eq!(listed, names, "every file is in a chunk, in order");
    assert!(chunks.len() >= 10, "{chunks:?}");
    assert_eq!(d["chunk_count"], chunks.len());
    // A binary file has no patch: its content is shown in no chunk.
    assert_eq!(d["not_shown"], json!(["assets/logo.png"]));
    names.retain(|n| n.ends_with(".rs"));
    assert!(names.len() >= 11);
}

#[test]
fn a_small_pr_has_no_not_shown_and_one_task() {
    let fake = FakeGithub::start();
    task(&fake, 7, r#","size":100"#);
    fake.pull_as_issue(42, "p");
    fake.pull(42, "p", "claude/x", "Closes #7");
    fake.pull_file(42, "src/t7/a.rs", "modified", 5, 0, Some("@@ a"));
    let d = pack(&fake);
    assert_eq!(d["not_shown"], json!([]));
    assert_eq!(d["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(d["unsized_tasks"], json!([]));
}
