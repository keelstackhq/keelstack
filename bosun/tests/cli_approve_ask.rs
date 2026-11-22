//! `bosun approve --list` never asks the owner (D-025): no `next`, no form, no "answer approve"
//! text. It returns the list and "the owner comments /bosun approve <sha> on the PR".

mod common;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::{Value, json};

const WAITING: &str = "high risk, owner approval needed: R-gov, R-ci";

fn head(prefix: &str) -> String {
    format!("{prefix:0<40}")
}

/// PR `n` at `head`, ready for an owner.
fn ready(fake: &FakeGithub, n: u64, title: &str, head: &str) {
    fake.pull(n, title, &format!("b{n}"), "");
    fake.pull_as_issue(n, title);
    fake.pull_set(
        n,
        "head",
        json!({"sha": head, "ref": format!("b{n}"), "label": format!("acme:b{n}")}),
    );
    fake.status(head, "risk-check", "failure", WAITING);
    fake.status(head, "review-verdict", "success", "approved by session_rev");
    fake.check_run(head, "ci-ok", "completed", Some("success"));
    fake.compare_file("base0000", head, &format!("governance/f{n}.toml"), "@@");
}

fn list(fake: &FakeGithub, origin: Option<&str>) -> Value {
    let mut cmd = bosun();
    cmd.env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_OWNER_TOKEN", "canary-owner-token-7f3a")
        .env("BOSUN_HTTP_MAX_SLEEP_MS", "0")
        .args(["approve", "--list"]);
    if let Some(o) = origin {
        cmd.env("BOSUN_ORIGIN", o);
    }
    let out = cmd.output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("canary-owner-token"));
    let v = one_object(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{v}");
    v
}

#[test]
fn list_returns_the_owner_comment_and_no_form_for_every_origin() {
    for origin in [None, Some("slack"), Some("spawned"), Some("routine")] {
        let fake = FakeGithub::start();
        let h = head("abc1234");
        ready(&fake, 12, "gov change", &h);
        let v = list(&fake, origin);
        assert_eq!(v["next"], json!([]), "{origin:?}: {v}");
        let item = &v["data"]["ready"][0];
        assert_eq!(item["pr"], json!(12));
        assert_eq!(item["owner_comment"], json!(format!("/bosun approve {h}")));
        assert_eq!(item["verdict"], json!("approved by session_rev"));
        assert!(item.get("ask").is_none(), "{v}");
        let step = v["data"]["owner_step"].as_str().unwrap();
        assert!(
            step.contains("the owner comments /bosun approve <sha> on the PR"),
            "{step}"
        );
        assert!(step.contains(&format!("`/bosun approve {h}`")), "{step}");
        let all = v.to_string();
        for old in [
            "AskUserQuestion",
            "answer \\\"approve",
            "ask_more",
            "\"handoff\"",
        ] {
            assert!(!all.contains(old), "{old} in {v}");
        }
    }
}

#[test]
fn list_names_every_ready_pr_not_just_three() {
    let fake = FakeGithub::start();
    for (n, p) in [(1, "a"), (2, "b"), (3, "c"), (4, "d")] {
        ready(&fake, n, "t", &head(p));
    }
    let v = list(&fake, None);
    let step = v["data"]["owner_step"].as_str().unwrap();
    for n in 1..=4 {
        assert!(step.contains(&format!("#{n}:")), "{step}");
        assert!(v["data"]["ready"][n - 1]["owner_comment"].is_string());
    }
}

#[test]
fn list_none_ready_no_next_and_no_owner_step() {
    let fake = FakeGithub::start();
    let v = list(&fake, None);
    assert_eq!(v["next"], json!([]));
    assert_eq!(v["data"]["ready"], json!([]));
    assert!(v["data"].get("owner_step").is_none());
}

#[test]
fn list_is_stable_and_never_prints_the_owner_token() {
    let fake = FakeGithub::start();
    ready(
        &fake,
        12,
        "leak canary-owner-token-7f3a here",
        &head("abc1234"),
    );
    // `list` asserts the token is absent from stdout.
    let a = list(&fake, None);
    let b = list(&fake, None);
    assert_eq!(a["data"]["owner_step"], b["data"]["owner_step"]);
}
