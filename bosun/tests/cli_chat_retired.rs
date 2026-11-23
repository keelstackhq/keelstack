//! Chat approvals are retired (D-025): a chat approval record never counts as the owner's approval.
mod common;

use common::fake::FakeGithub;
use common::run;
use serde_json::{Value, json};

const NOW: &str = "2026-11-10T10:00:00Z";
const LIVE: &str = "2026-11-10T11:00:00Z";
const BASE: &str = "base0000";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const OWNERS: &str = "[[owner]]\ngithub = \"ada\"\n";
const REG: u64 = 50;

/// Unix seconds of the time the fake stamps on every comment (2026-10-22T10:00:00Z).
const NOW_UNIX: i64 = 1_792_663_200;

fn chat_q(head: &str, session: &str, question: &str) -> String {
    format!(
        "/bosun approve {head} chat\nApproved by the owner in chat (session {session}, {NOW}).\n<!-- bosun:chat-approval v1 session={session} head={head} question={question} -->"
    )
}

fn chat(head: &str, session: &str) -> String {
    chat_q(head, session, &format!("1-0123456-{NOW_UNIX}"))
}

fn join(fake: &FakeGithub, session: &str, origin: &str, parent: Option<&str>, depth: u32) {
    let parent = parent.map_or(String::new(), |p| format!(",\"parent\":\"{p}\""));
    fake.comment(
        REG,
        &format!(
            "{{\"bosun\":\"join\",\"session\":\"{session}\",\"origin\":\"{origin}\"{parent},\"depth\":{depth},\"expires\":\"{LIVE}\"}}"
        ),
    );
}

/// High PR 1 at HEAD (a workflow change, owner class), owners at base, registry with root
/// `session_orch`.
fn high_pr(owners: bool) -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "");
    fake.pull_as_issue(1, "change");
    fake.pull_set(1, "head", json!({"sha": HEAD, "ref": "feature"}));
    if owners {
        fake.content("governance/owners.toml", BASE, OWNERS);
    }
    let patch = Some("@@ -0,0 +1,1 @@\n+x");
    fake.pull_file(1, ".github/workflows/ci.yml", "modified", 1, 0, patch);
    fake.issue(REG, "bosun registry", &["bosun:registry"], "");
    join(&fake, "session_orch", "local", None, 1);
    fake
}

/// A comment by `user` through the Claude app, as a cloud session posts it.
fn app_comment(fake: &FakeGithub, user: &str, text: &str) {
    fake.comment_by(1, user, text);
    fake.last_comment(|c| c.via_app = Some("claude".into()));
}

fn check(fake: &FakeGithub) -> (i32, Value) {
    run(fake, "session_test", NOW, &["risk-check", "1"])
}

fn approved(fake: &FakeGithub) -> Value {
    let (code, v) = check(fake);
    assert_eq!(code, 0, "{v}");
    v["data"]["approved_by"].clone()
}

fn refused(fake: &FakeGithub) {
    let (code, v) = check(fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
}

#[test]
fn a_chat_record_alone_stays_waiting_for_the_owner() {
    let fake = high_pr(true);
    app_comment(&fake, "ada", &chat(HEAD, "session_orch"));
    refused(&fake);
}

#[test]
fn a_chat_record_does_not_hide_the_owners_own_approval() {
    let fake = high_pr(true);
    app_comment(&fake, "ada", &chat(HEAD, "session_orch"));
    refused(&fake);
    fake.comment_by(1, "ada", &format!("/bosun approve {HEAD}"));
    assert_eq!(approved(&fake), "ada");
}

#[test]
fn the_fix_does_not_offer_a_chat_record() {
    let fake = high_pr(true);
    app_comment(&fake, "ada", &chat(HEAD, "session_orch"));
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    let text = v.to_string();
    assert!(
        text.contains("/bosun approve") && !text.contains("--chat"),
        "{text}"
    );
}

#[test]
fn approvals_chat_true_in_the_config_grants_nothing_under_any_policy() {
    for policy in [
        "owner",
        "owner-gated",
        "orchestrator",
        "autonomous",
        "quorum",
    ] {
        let fake = high_pr(true);
        let config =
            format!("project = \"widget\"\n\n[approvals]\nchat = true\npolicy = \"{policy}\"\n");
        fake.content(".bosun/bosun.toml", BASE, &config);
        fake.content(".bosun/bosun.toml", "refs/heads/main", &config);
        app_comment(&fake, "ada", &chat(HEAD, "session_orch"));
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{policy}: {v}");
        assert_eq!(v["data"]["approved_by"], Value::Null, "{policy}: {v}");
    }
}

#[test]
fn approve_chat_is_a_usage_error_with_one_json_object() {
    let fake = high_pr(true);
    let out = common::bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_OWNER_TOKEN", "owner-token")
        .args(["approve", &format!("1@{HEAD}"), "--chat"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let v = common::one_object(&out.stdout);
    assert_eq!(
        (v["ok"].clone(), v["exit_code"].clone()),
        (json!(false), json!(2))
    );
    assert!(
        !v["error"]["fix"].as_str().unwrap().trim().is_empty(),
        "{v}"
    );
}

#[test]
fn without_an_owners_file_a_plain_approve_comment_of_the_user_repo_owner_approves_nothing() {
    // No governance/owners.toml and the repo owner is the User "ada": ada's own plain
    // `/bosun approve <head>` comment names no owner, so it counts for no one (fail closed).
    let fake = high_pr(false);
    fake.repo_set("owner", json!({"login": "ada", "type": "User"}));
    fake.comment_by(1, "ada", &format!("/bosun approve {HEAD}"));
    refused(&fake);
}

#[test]
fn empty_owners_file_is_not_the_fallback() {
    // An owners file that exists but names nobody keeps the empty-owners halt (fail closed),
    // even for the repo owner's own plain `/bosun approve` comment.
    let fake = high_pr(false);
    fake.content("governance/owners.toml", BASE, "");
    fake.repo_set("owner", json!({"login": "ada", "type": "User"}));
    fake.comment_by(1, "ada", &format!("/bosun approve {HEAD}"));
    let (code, v) = check(&fake);
    assert_ne!(code, 0, "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
}
