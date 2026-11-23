//! `pr: one` in the plan header: every item is single, epics record `"pr":"one"`, and `data.big_pr`
//! summarises the one PR.

mod common;

use common::fake::FakeGithub;
use serde_json::{Value, json};
use std::path::Path;

const T0: &str = "2026-10-23T10:00:00Z";

fn plan_text(header: &str, shape: &str) -> String {
    format!(
        "# Plan: widget

project: widget
{header}
## M1: One
area: core
{shape}
### Tasks

- T1: a
  touch: src/b.rs, src/a.rs
  allow: skip

- T2: b
  touch: src/a.rs, tests/**
  risk: high

### Done when

- UC-M1-1: ok.

## M2: Two
area: core

### Tasks

- T1: c
  touch: src/c.rs

### Done when

- UC-M2-1: ok.
"
    )
}

fn run(fake: &FakeGithub, dir: &Path, cmd: &str) -> (i32, Value) {
    common::run(
        fake,
        "session_test",
        T0,
        &["-C", dir.to_str().unwrap(), "plan", cmd, "plan.md"],
    )
}

#[test]
fn pr_one_reports_big_pr_and_marks_epics() {
    let (tmp, fake) = (tempfile::tempdir().unwrap(), FakeGithub::start());
    std::fs::write(tmp.path().join("plan.md"), plan_text("pr: one\n", "")).unwrap();
    let (code, v) = run(&fake, tmp.path(), "check");
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        v["data"]["big_pr"],
        json!({"tasks": ["M1/T1", "M1/T2", "M2/T1"],
            "touch": ["src/a.rs", "src/b.rs", "src/c.rs", "tests/**"],
            "allow": {"M1/T1": ["skip"]}, "risk": "high"}),
        "{v}"
    );
    assert!(
        v["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["shape"] == "single")
    );
    let (code, v) = run(&fake, tmp.path(), "import");
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["big_pr"]["risk"], "high", "{v}");
    let st = fake.state.lock().unwrap();
    let epics: Vec<_> = (st.issues.iter())
        .filter(|i| i.body.contains("<!-- bosun:epic"))
        .collect();
    assert_eq!(epics.len(), 2);
    assert!(epics.iter().all(|e| e.body.contains(r#""pr":"one""#)));
    assert!(epics.iter().all(|e| e.body.contains(r#""shape":"single""#)));
}

#[test]
fn without_pr_one_there_is_no_big_pr() {
    let (tmp, fake) = (tempfile::tempdir().unwrap(), FakeGithub::start());
    std::fs::write(tmp.path().join("plan.md"), plan_text("", "")).unwrap();
    let (code, v) = run(&fake, tmp.path(), "check");
    assert_eq!(code, 0, "{v}");
    assert!(v["data"].get("big_pr").is_none(), "{v}");
}

#[test]
fn pr_one_rejects_bad_values_repeats_and_shape_split() {
    let (tmp, fake) = (tempfile::tempdir().unwrap(), FakeGithub::start());
    for (header, shape, want) in [
        ("pr: many\n", "", "pr `many` is not valid"),
        ("pr: one\npr: one\n", "", "`pr:` is given twice"),
        ("pr: one\n", "shape: split\n", "item M1 has `shape: split`"),
    ] {
        std::fs::write(tmp.path().join("plan.md"), plan_text(header, shape)).unwrap();
        let (code, v) = run(&fake, tmp.path(), "check");
        assert_eq!(code, 1, "{v}");
        assert!(v.to_string().contains(want), "{want}: {v}");
    }
}
