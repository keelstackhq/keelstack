//! A bundle PR under quorum (D-022): the author's lease must cover every open task the PR
//! closes, not just one of them (a bundle needs the union of its tasks' grants).
mod common;

use bosun::github::b64;
use common::fake::FakeGithub;
use common::{lease_note, run};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};

const BASE: &str = "base0000";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const MAIN: &str = "refs/heads/main";
const NOW: &str = "2026-12-05T10:00:00Z";
const PLAIN: &str = "@@ -0,0 +1,1 @@\n+x";
const GATE: &str = "src/feature/a.rs";
const RULES: &str = concat!(
    include_str!("../src/core/risk_default.toml"),
    "\n[[rule]]\nid = \"R-feature\"\nwhy = \"x\"\npaths = [\"src/feature/**\"]\n"
);

struct Key(Ed25519KeyPair);

fn key() -> Key {
    let rng = ring::rand::SystemRandom::new();
    let doc = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    Key(Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap())
}

fn config(policy: &str) -> String {
    format!("project = \"widget\"\n\n[approvals]\npolicy = \"{policy}\"\n")
}

fn verdict_of(session: &str, head: &str, major: u32, approved: bool) -> String {
    let r = json!({"bosun": "verdict", "session": session, "head_sha": head,
        "approved": approved, "confidence": 0.95, "major": major});
    format!("Review verdict: approve 0.95, 0 blocker.\n\n{r}")
}

/// PR 1 (Closes #7, leased by `session_w`, authored by `octocat`) at HEAD changing `file`;
/// the base policy is `policy`.
fn pr(k: &Key, file: &str, policy: &str) -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "Closes #7");
    fake.pull_as_issue(1, "change");
    fake.pull_set(1, "head", json!({"sha": HEAD, "ref": "feature"}));
    fake.issue(7, "task", &[], "task\n<!-- bosun:task {} -->");
    fake.comment_by(
        7,
        "octocat",
        &lease_note("session_w", "2026-12-06T12:00:00Z", "src/**"),
    );
    fake.content(
        "governance/owners.toml",
        BASE,
        "[[owner]]\ngithub = \"ada\"\n",
    );
    let public = b64::encode(k.0.public_key().as_ref());
    let keys = format!("[[key]]\nid = \"k1\"\ned25519 = \"{public}\"\n");
    fake.content(".bosun/approvers.toml", MAIN, &keys);
    fake.content(".bosun/bosun.toml", BASE, &config(policy));
    fake.content(".bosun/risk.toml", BASE, RULES);
    fake.content(".bosun/bosun.toml", MAIN, &config(policy));
    fake.pull_file(1, file, "modified", 1, 0, Some(PLAIN));
    fake
}

fn quorum(k: &Key) -> FakeGithub {
    pr(k, GATE, "quorum")
}

fn post_sig(fake: &FakeGithub, k: &Key) {
    let msg = format!("bosun-autonomous v1 acme/widget#1 {HEAD}");
    let s = b64::encode(k.0.sign(msg.as_bytes()).as_ref());
    let body = format!("/bosun approve {HEAD} autonomous\nbosun-sig ed25519 k1 {s}");
    fake.comment_by(1, "claude", &body);
    fake.last_comment(|c| c.via_app = Some("claude".into()));
}

fn post_verdict(fake: &FakeGithub, session: &str, head: &str) {
    fake.comment_by(
        1,
        "github-actions[bot]",
        &verdict_of(session, head, 0, true),
    );
}

fn check(fake: &FakeGithub) -> (i32, Value) {
    run(fake, "session_test", NOW, &["risk-check", "1"])
}

/// Signature plus the verdicts of `sessions` on `fake`.
fn with(fake: &FakeGithub, k: &Key, sessions: &[&str]) -> (i32, Value) {
    for s in sessions {
        post_verdict(fake, s, HEAD);
    }
    post_sig(fake, k);
    check(fake)
}

fn bundle(k: &Key, second_by: &str) -> FakeGithub {
    let fake = quorum(k);
    fake.pull_set(
        1,
        "body",
        json!("Closes #7\nCloses #8\n<!-- bosun:task-pr 7 -->"),
    );
    fake.issue(8, "task 2", &[], "task\n<!-- bosun:task {} -->");
    fake.comment_by(
        8,
        second_by,
        &lease_note("session_x", "2026-12-06T12:00:00Z", "src/**"),
    );
    fake
}

#[test]
fn a_bundle_with_every_task_leased_by_the_author_passes() {
    let k = key();
    let fake = bundle(&k, "octocat");
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_bundled_task_leased_by_someone_else_sends_it_to_the_owner() {
    let k = key();
    let fake = bundle(&k, "mallory");
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
}
