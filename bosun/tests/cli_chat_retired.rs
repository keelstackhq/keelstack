//! Chat approvals are retired (D-022): a chat approval record never counts as the owner's approval.
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
