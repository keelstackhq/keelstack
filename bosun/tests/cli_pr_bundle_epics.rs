//! One big PR (UC-WB-1): with the owner's `[approvals] bundle_across_epics = true` on the
//! default branch, `bosun pr <lead> --bundle` takes tasks of other single-shape epics.

mod common;

use std::process::Command;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::Value;

const BRANCH: &str = "claude/t11";
const BUILDING: &str = "bosun:state/building";
const READY: &str = "bosun:state/ready";
const LEASE: &str = r#"{"bosun":"lease","session":"session_test","origin":"spawned","expires":"2026-10-22T10:30:00Z","touch":["src/**","hoist.toml"]}"#;

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-q", "-b", BRANCH],
        vec![
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(&args)
                .status()
                .unwrap()
                .success()
        );
    }
    dir
}

fn run_pr(fake: &FakeGithub, extra: &[&str]) -> (i32, Value) {
    let dir = repo();
    let mut args = vec!["pr", "11"];
    args.extend_from_slice(extra);
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .arg("-C")
        .arg(dir.path())
        .args(&args)
        .output()
        .unwrap();
    (out.status.code().unwrap(), one_object(&out.stdout))
}

fn meta(epic: u64, extra: &str) -> String {
    format!(r#"Do it. <!-- bosun:task {{"item":"M1","task":"T","epic":{epic}{extra}}} -->"#)
}

/// Epic #10 (single), lead #11 (building, leased), ready bundle tasks #12, #13 and gated #14,
/// a task #15 of another single epic #20, and a split epic #30 with task #31.
fn world() -> FakeGithub {
    world_with_lead("")
}

fn world_with_lead(lead_extra: &str) -> FakeGithub {
    let fake = FakeGithub::start();
    fake.issue(
        10,
        "epic",
        &[],
        r#"<!-- bosun:epic {"item":"M1","shape":"single"} -->"#,
    );
    fake.issue(11, "lead", &[BUILDING], &meta(10, lead_extra));
    fake.comment(11, LEASE);
    fake.compare_file("main", BRANCH, "src/a.rs", "@@ -1 +1 @@");
    for n in [12, 13] {
        fake.issue(n, "t", &[READY], &meta(10, ""));
    }
    fake.issue(14, "g", &[READY], &meta(10, r#","gated":true"#));
    fake.issue(
        20,
        "epic2",
        &[],
        r#"<!-- bosun:epic {"item":"M2","shape":"single"} -->"#,
    );
    fake.issue(15, "o", &[READY], &meta(20, ""));
    fake.issue(
        30,
        "epic3",
        &[],
        r#"<!-- bosun:epic {"item":"M3","shape":"split"} -->"#,
    );
    fake.issue(31, "s", &[READY], &meta(30, ""));
    fake
}

const OPT_IN: &str = "[approvals]\nbundle_across_epics = true\n";

#[test]
fn without_the_owners_opt_in_another_epic_is_refused() {
    let fake = world();
    let (code, v) = run_pr(&fake, &["--bundle", "12", "--bundle", "15"]);
    assert_eq!(code, 1, "{v}");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(msg.contains("bundle_across_epics"), "{msg}");
    // The opt-in on the PR branch or the checkout counts for nothing: only the default branch.
    let fake = world();
    fake.content(".bosun/bosun.toml", BRANCH, OPT_IN);
    let (code, v) = run_pr(&fake, &["--bundle", "15"]);
    assert_eq!(code, 1, "{v}");
}

#[test]
fn with_the_opt_in_one_pr_closes_tasks_of_two_epics() {
    let fake = world();
    fake.content(".bosun/bosun.toml", "refs/heads/main", OPT_IN);
    let (code, v) = run_pr(&fake, &["--bundle", "12", "--bundle", "15"]);
    assert_eq!(code, 0, "{v}");
    let n = v["data"]["pr"].as_u64().unwrap();
    let body = fake.pull_body(n);
    assert!(
        body.starts_with("Closes #11\nCloses #12\nCloses #15\n"),
        "{body}"
    );
}

#[test]
fn the_opt_in_never_bundles_a_split_epics_task() {
    let fake = world();
    fake.content(".bosun/bosun.toml", "refs/heads/main", OPT_IN);
    let (code, v) = run_pr(&fake, &["--bundle", "31"]);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("single-shape"),
        "{v}"
    );
}
