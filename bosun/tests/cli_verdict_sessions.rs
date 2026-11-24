//! #52: review-verdict aggregates the latest verdict of each reviewer session on the head, and a
//! verdict moves every task a bundle PR closes.
mod common;

use common::fake::FakeGithub;
use common::{bosun, one_object, run};
use serde_json::{Value, json};

const T0: &str = "2026-10-22T10:00:00Z";
const MAIN: &str = "refs/heads/main";

fn setup(body: &str) -> FakeGithub {
    let fake = FakeGithub::start();
    fake.issue(7, "task", &["bosun:state/review"], "Do it.");
    fake.pull_as_issue(42, "the pr");
    fake.pull(42, "the pr", "claude/x", body);
    fake.content(".bosun/bosun.toml", MAIN, "");
    fake
}

fn verdict(v: &str, confidence: f64, sev: &[&str]) -> String {
    let findings: Vec<Value> = sev
        .iter()
        .map(|s| json!({"severity": s, "file": "src/a.rs", "line": 3, "claim": "bad", "evidence": "e"}))
        .collect();
    json!({"pr": 42, "head_sha": "sha42", "verdict": v, "confidence": confidence, "findings": findings})
        .to_string()
}

/// Session `session` posts and applies a verdict; the applied status state.
fn post(fake: &FakeGithub, session: &str, input: &str) -> (i32, Value) {
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_SESSION", session)
        .args(["verdict", "comment", "--from", "-"])
        .write_stdin(input.to_string())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", one_object(&out.stdout));
    let id = fake.comments_of(42).last().unwrap().id;
    run(
        fake,
        "session_ci",
        T0,
        &["verdict", "apply", &id.to_string()],
    )
}

fn last_state(fake: &FakeGithub) -> String {
    let st = fake.statuses_of("sha42");
    st.last().unwrap()["state"].as_str().unwrap().to_string()
}

#[test]
fn a_low_confidence_approve_does_not_undo_another_sessions_approval() {
    let fake = setup("Closes #7");
    let (code, v) = post(&fake, "session_kthf", &verdict("approve", 0.90, &[]));
    assert_eq!(code, 0, "{v}");
    assert_eq!(last_state(&fake), "success");
    let (code, v) = post(&fake, "session_0139", &verdict("approve", 0.80, &[]));
    assert_eq!(code, 0, "{v}");
    assert_eq!(last_state(&fake), "success", "{v}");
    assert_eq!(v["data"]["approved"], false);
    assert_eq!(v["data"]["sessions"], json!(["session_kthf"]));
}

#[test]
fn the_order_of_the_verdicts_does_not_matter() {
    let fake = setup("Closes #7");
    post(&fake, "session_0139", &verdict("approve", 0.80, &[]));
    assert_eq!(last_state(&fake), "failure");
    let (code, v) = post(&fake, "session_kthf", &verdict("approve", 0.90, &[]));
    assert_eq!(code, 0, "{v}");
    assert_eq!(last_state(&fake), "success");
}

#[test]
fn any_sessions_blocker_or_request_changes_keeps_it_red() {
    for blocking in [
        verdict("approve", 0.95, &["blocker"]),
        verdict("request-changes", 0.9, &[]),
    ] {
        let fake = setup("Closes #7");
        post(&fake, "session_b", &blocking);
        let (code, v) = post(&fake, "session_a", &verdict("approve", 0.95, &[]));
        assert_eq!(code, 0, "{v}");
        assert_eq!(last_state(&fake), "failure", "{v}");
    }
}

#[test]
fn a_sessions_newer_verdict_replaces_its_older_one() {
    let fake = setup("Closes #7");
    post(
        &fake,
        "session_b",
        &verdict("request-changes", 0.9, &["major"]),
    );
    post(&fake, "session_a", &verdict("approve", 0.95, &[]));
    assert_eq!(last_state(&fake), "failure");
    // session_b's fix review approves: its old request-changes no longer counts.
    post(&fake, "session_b", &verdict("approve", 0.90, &[]));
    assert_eq!(last_state(&fake), "success");
}

#[test]
fn a_verdict_moves_every_task_the_bundle_closes() {
    let task = |id: &str| {
        format!("<!-- bosun:task {{\"item\":\"M1\",\"task\":\"{id}\",\"touch\":[\"src/**\"]}} -->")
    };
    let fake = setup("Closes #7\nCloses #8\nCloses #9\nCloses #10");
    fake.issue(8, "M1/T2", &["bosun:state/review"], &task("T2"));
    fake.issue(9, "M2/T1", &["bosun:state/review"], &task("T1"));
    // A plain issue the PR also closes is no task: it keeps its labels.
    fake.issue(10, "plain", &[], "words");
    let (code, v) = post(
        &fake,
        "session_a",
        &verdict("request-changes", 0.9, &["major"]),
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["tasks_moved"], json!([7, 8, 9]));
    for n in [7, 8, 9] {
        assert!(
            fake.labels_of(n)
                .contains(&"bosun:state/fixing".to_string()),
            "#{n}"
        );
    }
    assert!(fake.labels_of(10).is_empty());
}
