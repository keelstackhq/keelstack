//! #11 (main-red #9, run 45128620888): `ci-ok` went red on a push to main because the root job
//! `ci-repo` failed in `cargo deny check` (RUSTSEC-2026-0592 on an unchanged Cargo.lock). Every
//! `ci-<folder>` check was green. No PR gate could have caught it: the advisory database changed
//! after the last merge, and a PR that touches no Rust skips `cargo deny`. The loop that finds it
//! is main-health (every push to main, and weekly). What the gate must do in that state: block
//! every non-mainfix PR (a root job is red, so no folder is safe), let the mainfix PR through, and
//! open the "Which gate missed this?" task for the run.
mod common;

use common::fake::FakeGithub;
use common::run;
use serde_json::{Value, json};

const NOW: &str = "2026-11-23T10:00:00Z";
const MAIN: &str = "refs/heads/main";
const RUN: &str = "https://github.com/keelstackhq/keelstack/actions/runs/45128620888";
const FOLDERS: &str = "[[folders]]\npath = \"bosun\"\n\n[[folders]]\npath = \"hoist\"\n";

/// Main as run 45128620888 left it: root job `ci-repo` and `ci-ok` red, every folder green.
fn red_main() -> FakeGithub {
    let fake = FakeGithub::start();
    fake.issue(9, "main is red", &["bosun:main-red"], "ci-ok failed");
    fake.content(".bosun/bosun.toml", MAIN, FOLDERS);
    for (name, c) in [
        ("ci-bosun", "success"),
        ("ci-hoist", "success"),
        ("ci-repo", "failure"),
        ("ci-ok", "failure"),
    ] {
        fake.check_run("main", name, "completed", Some(c));
    }
    fake
}

fn main_open(fake: &FakeGithub, pr: u64, files: &[&str]) -> (i32, Value) {
    fake.pull(pr, "change", &format!("f{pr}"), "No link.");
    fake.pull_set(pr, "changed_files", json!(files.len()));
    for f in files {
        fake.pull_file(pr, f, "modified", 1, 0, Some("@@ -1 +1 @@\n+x"));
    }
    run(
        fake,
        "session_a",
        NOW,
        &["gate", "main-open", "--pr", &pr.to_string()],
    )
}

#[test]
fn a_red_root_job_blocks_a_pr_in_any_folder_and_one_outside_them() {
    let fake = red_main();
    let (code, v) = main_open(&fake, 10, &["hoist/src/a.rs"]);
    assert_eq!(code, 4, "{v}");
    // The docs-only push that went red was a root file: it touches every folder.
    let (code, v) = main_open(&fake, 11, &["plans/port-integrity.md"]);
    assert_eq!(code, 4, "{v}");
}

#[test]
fn the_mainfix_that_bumps_the_lockfile_passes() {
    let fake = red_main();
    fake.pull(12, "bump url", "fix", "No link.");
    fake.pull_set(12, "labels", json!([{"name": "bosun:kind/mainfix"}]));
    fake.pull_set(12, "changed_files", json!(1));
    fake.pull_file(12, "Cargo.lock", "modified", 1, 1, Some("@@ -1 +1 @@\n+x"));
    let (code, v) = run(
        &fake,
        "session_a",
        NOW,
        &["gate", "main-open", "--pr", "12"],
    );
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_failed_run_opens_the_gate_missed_task_for_that_run() {
    let fake = FakeGithub::start();
    let (code, v) = run(
        &fake,
        "session_a",
        NOW,
        &["gate-missed", "--run", RUN, "--range", "good..722bad4"],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["created"], true);
    let body = fake
        .state
        .lock()
        .unwrap()
        .issues
        .iter()
        .map(|i| i.body.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        body.contains(RUN) && body.contains("good..722bad4"),
        "{body}"
    );
}
