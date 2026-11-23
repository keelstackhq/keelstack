//! A PR that changes a trust file (`governance/owners.toml`) is approved only by the owner's own
//! plain `/bosun approve <head>` comment (restored from the deleted `chat_never_counts_on_trust_file_pr`,
//! D-025: a chat record is plain text and never counts).
mod common;

use common::fake::FakeGithub;
use common::run;
use serde_json::{Value, json};

const NOW: &str = "2026-11-10T10:00:00Z";
const BASE: &str = "base0000";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const OWNERS: &str = "[[owner]]\ngithub = \"ada\"\n";

/// PR 1 at HEAD that adds an owner (`eve`) to `governance/owners.toml`; owners at base.
fn trust_pr() -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "");
    fake.pull_as_issue(1, "change");
    fake.pull_set(1, "head", json!({"sha": HEAD, "ref": "feature"}));
    fake.content("governance/owners.toml", BASE, OWNERS);
    let patch = Some("@@ -1,1 +1,2 @@\n [[owner]]\n+github = \"eve\"");
    fake.pull_file(1, "governance/owners.toml", "modified", 1, 0, patch);
    fake
}

fn check(fake: &FakeGithub) -> (i32, Value) {
    run(fake, "session_test", NOW, &["risk-check", "1"])
}

#[test]
fn the_owners_plain_comment_approves_a_trust_file_pr() {
    let fake = trust_pr();
    // No approval yet: the PR waits for the owner (exit 5).
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
    // A chat-approval record is plain text now and counts for nothing.
    fake.comment_by(
        1,
        "ada",
        &format!(
            "/bosun approve {HEAD} chat\n<!-- bosun:chat-approval v1 session=session_orch head={HEAD} question=1-0123456-1792663200 -->"
        ),
    );
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
    // The owner's own `/bosun approve <head>` comment on the exact head counts.
    fake.comment_by(1, "ada", &format!("/bosun approve {HEAD}"));
    let (code, v) = check(&fake);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["approved_by"], json!("ada"), "{v}");
}

#[test]
fn another_login_or_another_head_does_not_approve_a_trust_file_pr() {
    let fake = trust_pr();
    fake.comment_by(1, "mallory", &format!("/bosun approve {HEAD}"));
    fake.comment_by(
        1,
        "ada",
        "/bosun approve 1111111111111111111111111111111111111111",
    );
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
}
