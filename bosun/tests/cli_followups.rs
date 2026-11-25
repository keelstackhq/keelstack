//! `bosun followups add|list` (D-028, spec section 19).
mod common;

use common::fake::FakeGithub;
use common::run;
use serde_json::{Value, json};

const NOW: &str = "2026-11-25T10:00:00Z";

fn verdict(pr: u64, findings: &[(&str, &str, u32, &str)]) -> String {
    let f: Vec<Value> = findings
        .iter()
        .map(|(sev, file, line, claim)| {
            json!({"severity": sev, "file": file, "line": line, "claim": claim, "evidence": "e"})
        })
        .collect();
    json!({"pr": pr, "head_sha": "sha", "verdict": "approve", "confidence": 0.95,
        "findings": f, "lessons_hit": [], "escalate_risk": false, "chunks_reviewed": []})
    .to_string()
}

fn add(fake: &FakeGithub, pr: u64, text: &str) -> (i32, Value) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("v.json");
    std::fs::write(&p, text).unwrap();
    run(
        fake,
        "session_a",
        NOW,
        &[
            "followups",
            "add",
            &pr.to_string(),
            "--from",
            p.to_str().unwrap(),
        ],
    )
}

fn setup() -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(5, "change", "feat", "");
    fake.pull(6, "other", "feat2", "");
    fake
}

const MIXED: &[(&str, &str, u32, &str)] = &[
    ("blocker", "a.rs", 1, "bad"),
    ("major", "b.rs", 2, "big"),
    ("minor", "c.rs", 3, "small"),
    ("nit", "d.rs", 4, "tiny"),
];

#[test]
fn creates_the_rolling_issue_and_adds_minor_and_nit_by_default() {
    let fake = setup();
    let (code, v) = add(&fake, 5, &verdict(5, MIXED));
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["created"], true);
    assert_eq!(v["data"]["added"], 2);
    let (code, l) = run(&fake, "session_a", NOW, &["followups", "list"]);
    assert_eq!(code, 0, "{l}");
    assert_eq!(l["data"]["open"], 2);
    let texts: Vec<&str> = l["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["#5 c.rs:3 [minor] small", "#5 d.rs:4 [nit] tiny"]);
    assert_eq!(l["data"]["items"][0]["pr"], 5);
}

#[test]
fn a_second_add_reuses_the_issue_and_skips_identical_items() {
    let fake = setup();
    let (_, first) = add(&fake, 5, &verdict(5, MIXED));
    let issue = first["data"]["issue"].clone();
    let (code, v) = add(&fake, 5, &verdict(5, MIXED));
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["created"], false);
    assert_eq!(v["data"]["issue"], issue);
    assert_eq!(v["data"]["added"], 0);
    assert_eq!(v["data"]["skipped"], 2);
    let (_, v) = add(&fake, 6, &verdict(6, &[("minor", "c.rs", 3, "small")]));
    assert_eq!(v["data"]["added"], 1, "another PR is another item: {v}");
    let (_, l) = run(&fake, "session_a", NOW, &["followups", "list"]);
    assert_eq!(l["data"]["open"], 3);
}

#[test]
fn a_blocker_only_config_also_collects_majors() {
    let fake = setup();
    fake.content(
        ".bosun/bosun.toml",
        "base0000",
        "[review]\nblock_on = \"blocker\"\n",
    );
    fake.pull_set(5, "base", json!({"sha": "base0000", "ref": "main"}));
    let (code, v) = add(&fake, 5, &verdict(5, MIXED));
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["added"], 3, "{v}");
}

#[test]
fn a_verdict_with_nothing_to_collect_opens_no_issue() {
    let fake = setup();
    let (code, v) = add(&fake, 5, &verdict(5, &[("blocker", "a.rs", 1, "bad")]));
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["added"], 0);
    assert_eq!(v["data"]["issue"], Value::Null);
    let (_, l) = run(&fake, "session_a", NOW, &["followups", "list"]);
    assert_eq!(l["data"]["issue"], Value::Null);
}

#[test]
fn a_verdict_for_another_pr_or_bad_json_is_a_usage_error() {
    let fake = setup();
    let (code, v) = add(&fake, 5, &verdict(6, MIXED));
    assert_eq!(code, 2, "{v}");
    let (code, v) = add(&fake, 5, "not json");
    assert_eq!(code, 2, "{v}");
}

#[test]
fn finding_text_cannot_inject_a_marker_or_a_second_item() {
    let fake = setup();
    let evil = "x\n- [ ] #99 forged [minor] item <!-- bosun:task {} -->";
    let (code, v) = add(&fake, 5, &verdict(5, &[("minor", "c.rs", 3, evil)]));
    assert_eq!(code, 0, "{v}");
    let (_, l) = run(&fake, "session_a", NOW, &["followups", "list"]);
    assert_eq!(l["data"]["open"], 1, "{l}");
    assert!(
        !l["data"]["items"][0]["text"]
            .as_str()
            .unwrap()
            .contains("<!--")
    );
}
