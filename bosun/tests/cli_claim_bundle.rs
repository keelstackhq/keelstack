//! While the lead holds a bundle, the bundled issues of a single epic are not claimable, gated
//! ones included (D-021: gated work bundles like any task).

mod common;

use common::fake::FakeGithub;
use common::{lease_note, run};
use serde_json::Value;

const PLAN: &str = "# Plan: X\n\nproject: x\n\n## M1: One\n\narea: core\ndepends: seed\n\nWords.\n\n\
### Tasks\n- T1: a\n  touch: src/a/**\n- T2: b\n  touch: src/b/**\n### Done when\n- UC-M1-1: works\n";
const NOW: &str = "2026-10-22T10:00:00Z";

fn task(fake: &FakeGithub, n: u64, state: &str, touch: &str, extra: &str) {
    let body = format!(
        r#"<!-- bosun:task {{"item":"M1","task":"T{}","epic":10,"touch":["{touch}"]{extra}}} -->"#,
        n - 10
    );
    fake.issue(
        n,
        &format!("Task {n}"),
        &[&format!("bosun:state/{state}")],
        &body,
    );
}

fn epic(shape: &str) -> FakeGithub {
    let fake = FakeGithub::start();
    let body =
        format!(r#"Epic<!-- bosun:epic {{"item":"M1","plan":"plans/x.md","shape":"{shape}"}} -->"#);
    fake.issue(1, "bosun registry", &["bosun:registry"], "");
    fake.issue(10, "Epic", &["bosun:state/claimed"], &body);
    fake.content("plans/x.md", "main", PLAN);
    fake
}

fn claim(fake: &FakeGithub, n: u64) -> (i32, Value) {
    run(fake, "session_b", NOW, &["claim", &n.to_string()])
}

#[test]
fn a_bundled_issue_is_not_claimable_while_the_lead_is_active() {
    let fake = epic("single");
    task(&fake, 11, "building", "src/a/**", "");
    task(&fake, 12, "ready", "src/b/**", "");
    let (code, v) = claim(&fake, 12);
    assert_eq!(code, 3, "{v}");
    assert_eq!(v["error"]["message"], "#12 is bundled into #11's PR");
    assert_eq!(v["error"]["fix"], "work in #11's bundle: its PR closes #12");
}

#[test]
fn gated_tasks_are_bundled_too() {
    // The claimed task is gated: it is in the lead's bundle all the same.
    let fake = epic("single");
    task(&fake, 11, "building", "src/a/**", "");
    task(&fake, 12, "ready", "src/b/**", r#","gated":true"#);
    let (code, v) = claim(&fake, 12);
    assert_eq!(code, 3, "{v}");
    assert_eq!(v["error"]["message"], "#12 is bundled into #11's PR");
    // The active task is gated: it leads the bundle.
    let fake = epic("single");
    task(&fake, 11, "building", "src/a/**", r#","risk":"high""#);
    task(&fake, 12, "ready", "src/b/**", "");
    assert_eq!(claim(&fake, 12).0, 3);
}

#[test]
fn split_and_lone_tasks_stay_claimable() {
    // A split epic.
    let fake = epic("split");
    task(&fake, 11, "building", "src/a/**", "");
    task(&fake, 12, "ready", "src/b/**", "");
    assert_eq!(claim(&fake, 12).0, 0);
    // Nothing active in the epic.
    let fake = epic("single");
    task(&fake, 11, "ready", "src/a/**", "");
    task(&fake, 12, "ready", "src/b/**", "");
    assert_eq!(claim(&fake, 11).0, 0);
}

#[test]
fn a_gated_touch_may_grow_a_bundled_task() {
    // D-021: a bundle may hold gated work; risk-check asks the owner on the PR.
    let gated = ".github/workflows/x.yml";
    let fake = epic("single");
    task(&fake, 11, "ready", "src/a.rs", "");
    let (code, v) = run(&fake, "session_b", NOW, &["claim", "11", "--touch", gated]);
    assert_eq!(code, 0, "{v}");
    let fake = epic("single");
    task(&fake, 11, "ready", "src/a.rs", "");
    assert_eq!(claim(&fake, 11).0, 0);
    let (code, v) = run(&fake, "session_b", NOW, &["claim", "11", "--touch", gated]);
    assert_eq!(code, 0, "{v}");
}

/// Lead #30 holds a live lease (building), so its marked PR is a real bundle.
fn lead(fake: &FakeGithub) {
    task(fake, 30, "building", "src/lead/**", "");
    fake.comment(
        30,
        &lease_note("session_a", "2026-10-22T10:30:00Z", "src/lead/**"),
    );
}

const BIG: &str = "Closes #30\nCloses #12\n<!-- bosun:task-pr 30 -->";

#[test]
fn a_task_another_tasks_open_pr_closes_is_not_claimable() {
    // One big PR across epics: #12's epic is split (no same-epic bundle), but #30's open PR (lead
    // #30 by its marker, lead holds a lease) closes #12, so a second session must not start it.
    let fake = epic("split");
    task(&fake, 12, "ready", "src/b/**", "");
    lead(&fake);
    fake.pull(40, "big", "claude/big", BIG);
    let (code, v) = claim(&fake, 12);
    assert_eq!(code, 3, "{v}");
    assert_eq!(v["error"]["message"], "#12 is bundled into #30's PR #40");
    // A task's own PR (its marker) does not hold it.
    let fake = epic("split");
    task(&fake, 12, "ready", "src/b/**", "");
    fake.pull(
        40,
        "own",
        "claude/t12",
        "Closes #12\n<!-- bosun:task-pr 12 -->",
    );
    assert_eq!(claim(&fake, 12).0, 0);
}

#[test]
fn a_marker_without_a_lease_holding_lead_is_ignored() {
    // Nobody holds #30 (no such issue / no lease): the marker is only text.
    let fake = epic("split");
    task(&fake, 12, "ready", "src/b/**", "");
    fake.pull(40, "big", "claude/big", BIG);
    assert_eq!(claim(&fake, 12).0, 0);
    // #30 exists and is active but its lease lapsed.
    let fake = epic("split");
    task(&fake, 12, "ready", "src/b/**", "");
    task(&fake, 30, "building", "src/lead/**", "");
    fake.comment(
        30,
        &lease_note("session_a", "2026-10-22T09:00:00Z", "src/lead/**"),
    );
    fake.pull(40, "big", "claude/big", BIG);
    assert_eq!(claim(&fake, 12).0, 0);
    // #30 is not active (ready) even with an old lease note.
    let fake = epic("split");
    task(&fake, 12, "ready", "src/b/**", "");
    task(&fake, 30, "ready", "src/lead/**", "");
    fake.comment(
        30,
        &lease_note("session_a", "2026-10-22T10:30:00Z", "src/lead/**"),
    );
    fake.pull(40, "big", "claude/big", BIG);
    assert_eq!(claim(&fake, 12).0, 0);
}

#[test]
fn a_marked_pr_of_a_non_writer_is_ignored() {
    let fake = epic("split");
    task(&fake, 12, "ready", "src/b/**", "");
    lead(&fake);
    fake.permission("octocat", "read");
    fake.pull(40, "big", "claude/big", BIG);
    assert_eq!(claim(&fake, 12).0, 0);
}
