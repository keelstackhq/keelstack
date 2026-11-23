//! The keelstack big PR (one PR closing ~45 issues across folders): risk-check reads every task
//! it closes, needs the owner (the strictest class wins: the plan's `risk: high` tasks and the
//! governance paths), and passes once the owner approves the exact head.

mod common;

use common::fake::FakeGithub;
use common::run;
use serde_json::Value;

const NOW: &str = "2026-10-22T10:00:00Z";
const BASE: &str = "base0000";
const PLAIN: [u64; 44] = [
    6, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 31, 32, 33,
    34, 36, 37, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 64,
];

fn task(id: &str, risk: &str) -> String {
    let (item, t) = id.split_once('/').unwrap();
    format!(r#"<!-- bosun:task {{"item":"{item}","task":"{t}","touch":["**"]{risk}}} -->"#)
}

/// PR #100 closing the 44 plain issues and task issues #201 (WB/T1, high), #202 (WH/T1, normal)
/// and #205 (IN/T2, high), changing files in five folders.
fn big(files: &[&str]) -> FakeGithub {
    big_with(files, r#","risk":"high""#)
}

fn big_with(files: &[&str], high: &str) -> FakeGithub {
    let fake = FakeGithub::start();
    let mut body = String::from("One big PR.\n\n");
    for n in PLAIN.iter().chain(&[201, 202, 205]) {
        body.push_str(&format!("Closes #{n}\n"));
    }
    body.push_str("<!-- bosun:task-pr 205 -->\n");
    fake.pull(100, "one big PR", "claude/big-pr", &body);
    for n in PLAIN {
        fake.issue(n, &format!("issue {n}"), &[], "words");
    }
    fake.issue(201, "WB/T1", &[], &task("WB/T1", high));
    fake.issue(202, "WH/T1", &[], &task("WH/T1", ""));
    fake.issue(205, "IN/T2", &[], &task("IN/T2", high));
    fake.content(
        "governance/owners.toml",
        BASE,
        "[[owner]]\ngithub = \"ada\"\n",
    );
    for f in files {
        fake.pull_file(100, f, "modified", 1, 0, Some("@@ -0,0 +1,1 @@\n+x"));
    }
    fake
}

fn rule<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
    (v["data"]["matches"].as_array()?.iter()).find(|m| m["rule"] == id)
}

const FILES: [&str; 5] = [
    "bosun/src/cli/pr_bundle.rs",
    "hull/template/.claude/hooks/guard.mjs",
    "keel/src/main.rs",
    "hoist/src/dns.rs",
    "site/src/index.ts",
];

#[test]
fn the_big_pr_needs_the_owner_for_every_high_task() {
    let fake = big(&FILES);
    let (code, v) = run(&fake, "session_test", NOW, &["risk-check", "100"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["level"], "high", "{v}");
    assert_eq!(v["data"]["requirement"], "owner", "{v}");
    let plan = rule(&v, "R-plan").expect("R-plan").to_string();
    // Every high task counts, not only the lead or the first link.
    assert!(plan.contains("#201") && plan.contains("#205"), "{plan}");
    assert!(!plan.contains("#202"), "{plan}");
}

#[test]
fn ordinary_code_with_no_high_task_is_normal() {
    let fake = big_with(&["bosun/src/cli/pr_bundle.rs"], "");
    let (code, v) = run(&fake, "session_test", NOW, &["risk-check", "100"]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn the_owners_approval_on_the_head_passes_the_big_pr() {
    let fake = big(&FILES);
    fake.review(100, "ada", "APPROVED", "sha100");
    let (code, v) = run(&fake, "session_test", NOW, &["risk-check", "100"]);
    assert_eq!(code, 0, "{v}");
    let st = fake.statuses_of("sha100");
    let s = st
        .iter()
        .rev()
        .find(|s| s["context"] == "risk-check")
        .unwrap();
    assert_eq!(s["state"], "success", "{v}");
}

#[test]
fn a_bundle_without_a_high_task_is_still_judged_by_its_paths() {
    // The strictest class wins: a governance path makes it the owner's whatever the tasks say.
    let fake = big_with(&["governance/owners.toml"], "");
    let (code, v) = run(&fake, "session_test", NOW, &["risk-check", "100"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["requirement"], "owner", "{v}");
    assert!(rule(&v, "R-plan").is_none(), "{v}");
}
