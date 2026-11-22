//! A quorum signature carries over a clean merge of main only with two clean verdicts on the new
//! head (D-022): the signature names the old head, the verdicts must be re-counted on the new one.
mod common;

use bosun::core::records::Record;
use bosun::github::b64;
use common::fake::FakeGithub;
use common::{lease_note, run};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};

const B: &str = "b000000000000000000000000000000000000000";
const H: &str = "a000000000000000000000000000000000000000";
const M: &str = "c000000000000000000000000000000000000000";
const H2: &str = "d000000000000000000000000000000000000000";
const D: &str = "e000000000000000000000000000000000000000";
const TB: &str = "f0b0000000000000000000000000000000000000";
const TM: &str = "f0c0000000000000000000000000000000000000";
const TH: &str = "f0a0000000000000000000000000000000000000";
const TH2: &str = "f0d0000000000000000000000000000000000000";
const NOW: &str = "2026-10-22T10:00:00Z";
const RULES: &str = concat!(
    include_str!("../src/core/risk_default.toml"),
    "\n[[rule]]\nid = \"R-feature\"\nwhy = \"x\"\npaths = [\"src/pr.rs\"]\n"
);

type Ent = (&'static str, &'static str, &'static str, &'static str);

fn tree(main: &'static str, pr: &'static str, dir: &'static str) -> Vec<Ent> {
    vec![
        ("src", "040000", "tree", dir),
        ("src/main.rs", "100644", "blob", main),
        ("src/pr.rs", "100644", "blob", pr),
    ]
}

fn cmp(fake: &FakeGithub, (a, b): (&str, &str), status: &str, base: Option<&str>, file: &str) {
    let key = format!("{a}...{b}");
    let f = json!({"filename": file, "status": "modified", "additions": 1, "deletions": 0});
    let mut s = fake.state.lock().unwrap();
    let files = if file.is_empty() { vec![] } else { vec![f] };
    s.compares.insert(key.clone(), files);
    s.compare_statuses.insert(key.clone(), status.into());
    if let Some(x) = base {
        s.compare_bases.insert(key, x.into());
    }
}

struct Key(Ed25519KeyPair);

/// Quorum PR 1 (task #7 leased by `session_a`, changes `src/pr.rs`): H2 merges main's M into H.
/// Nothing is signed or reviewed yet.
fn world() -> (FakeGithub, Key) {
    let rng = ring::rand::SystemRandom::new();
    let doc = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let key = Key(Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap());
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feat", "Closes #7");
    fake.pull_as_issue(1, "change");
    fake.pull_set(1, "head", json!({"sha": H2, "ref": "feat"}));
    fake.pull_set(1, "base", json!({"sha": D, "ref": "main"}));
    fake.issue(7, "the task", &[], "Do it.\n<!-- bosun:task {} -->");
    fake.comment_by(
        7,
        "octocat",
        &lease_note("session_a", "2026-10-22T10:30:00Z", "src/**"),
    );
    for (sha, t, parents) in [
        (H2, TH2, vec![H, M]),
        (H, TH, vec![B]),
        (M, TM, vec![B]),
        (B, TB, vec![]),
    ] {
        fake.git_commit(sha, t, &parents);
    }
    for (t, e) in [
        (TB, tree("m1", "p0", "d0")),
        (TM, tree("m2", "p0", "d1")),
        (TH, tree("m1", "p1", "d2")),
        (TH2, tree("m2", "p1", "d3")),
    ] {
        fake.git_tree(t, &e, false);
    }
    cmp(&fake, (D, "refs/heads/main"), "identical", None, "");
    cmp(&fake, (M, D), "ahead", None, "");
    cmp(&fake, (D, H), "diverged", Some(B), "");
    cmp(&fake, (B, M), "ahead", None, "src/main.rs");
    cmp(&fake, (B, H), "ahead", None, "src/pr.rs");
    fake.content("governance/owners.toml", D, "[[owner]]\ngithub = \"ada\"\n");
    let public = b64::encode(key.0.public_key().as_ref());
    let keys = format!("[[key]]\nid = \"k1\"\ned25519 = \"{public}\"\n");
    fake.content(".bosun/approvers.toml", "refs/heads/main", &keys);
    let config = "project = \"widget\"\n\n[approvals]\npolicy = \"quorum\"\n";
    fake.content(".bosun/bosun.toml", D, config);
    fake.content(".bosun/bosun.toml", "refs/heads/main", config);
    fake.content(".bosun/risk.toml", D, RULES);
    fake.pull_file(
        1,
        "src/pr.rs",
        "modified",
        1,
        0,
        Some("@@ -0,0 +1,1 @@\n+x"),
    );
    (fake, key)
}

fn verdict(fake: &FakeGithub, sha: &str, session: &str) {
    let r = Record::Verdict {
        session: session.into(),
        head_sha: sha.into(),
        approved: true,
        confidence: Some(0.95),
        major: Some(0),
        model: None,
        relayed_by: None,
    };
    let body = format!("Review verdict: x.\n\n{}", r.to_line());
    fake.comment_by(1, "github-actions[bot]", &body);
}

fn two(fake: &FakeGithub, sha: &str) {
    verdict(fake, sha, "session_r");
    verdict(fake, sha, "session_s");
}

fn sign(fake: &FakeGithub, k: &Key, sha: &str) {
    let msg = format!("bosun-autonomous v1 acme/widget#1 {sha}");
    let s = b64::encode(k.0.sign(msg.as_bytes()).as_ref());
    let body = format!("/bosun approve {sha} autonomous\nbosun-sig ed25519 k1 {s}");
    fake.comment_at(1, "claude", &body, "2026-10-22T09:00:00Z");
    fake.last_comment(|c| c.via_app = Some("claude".into()));
}

fn go(fake: &FakeGithub) -> (i32, Value) {
    run(fake, "session_test", NOW, &["risk-check", "1"])
}

#[test]
fn a_signature_on_h_with_two_verdicts_on_h_and_on_the_new_head_carries() {
    let (fake, k) = world();
    sign(&fake, &k, H);
    two(&fake, H);
    two(&fake, H2);
    let (code, v) = go(&fake);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["approval_carried_from"], H, "{v}");
}

#[test]
fn a_carried_signature_without_two_clean_verdicts_on_the_new_head_does_not_count() {
    // None, one, the same session twice, a verdict of the author's session: each is below two.
    for sessions in [
        &[][..],
        &["session_r"][..],
        &["session_r", "session_r"][..],
        &["session_r", "session_a"][..],
    ] {
        let (fake, k) = world();
        sign(&fake, &k, H);
        two(&fake, H);
        for s in sessions {
            verdict(&fake, H2, s);
        }
        let (code, v) = go(&fake);
        assert_eq!(code, 5, "{sessions:?}: {v}");
        assert_eq!(
            v["data"]["approval_carried_from"],
            Value::Null,
            "{sessions:?}: {v}"
        );
    }
}

/// An owner-role key signature (v1, v2) on `h` is a key, not the owner (D-022): its carry also
/// needs the two clean verdicts counted again on the new head.
fn sign_owner_key(fake: &FakeGithub, k: &Key, sha: &str, v2: bool) {
    let (msg, first) = if v2 {
        (
            format!("bosun-approve v2 acme/widget#1 {sha} after-review 7"),
            format!("/bosun approve {sha} after-review 7"),
        )
    } else {
        (
            format!("bosun-approve v1 acme/widget#1 {sha}"),
            format!("/bosun approve {sha}"),
        )
    };
    let s = b64::encode(k.0.sign(msg.as_bytes()).as_ref());
    let body = format!("{first}\nbosun-sig ed25519 k1 {s}");
    fake.comment_at(1, "claude", &body, "2026-10-22T09:00:00Z");
    fake.last_comment(|c| c.via_app = Some("claude".into()));
}

#[test]
fn a_carried_owner_role_key_signature_needs_two_verdicts_on_the_new_head() {
    for v2 in [false, true] {
        for sessions in [&[][..], &["session_r"][..]] {
            let (fake, k) = world();
            sign_owner_key(&fake, &k, H, v2);
            two(&fake, H);
            for s in sessions {
                verdict(&fake, H2, s);
            }
            let (code, v) = go(&fake);
            assert_eq!(code, 5, "v2={v2} {sessions:?}: {v}");
            assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
            assert_eq!(v["data"]["approval_carried_from"], Value::Null, "{v}");
        }
        let (fake, k) = world();
        sign_owner_key(&fake, &k, H, v2);
        two(&fake, H);
        two(&fake, H2);
        let (code, v) = go(&fake);
        assert_eq!(code, 0, "v2={v2}: {v}");
        assert_eq!(v["data"]["approval_carried_from"], H, "{v}");
    }
}

#[test]
fn while_halted_a_carried_signature_does_not_count() {
    let (fake, k) = world();
    fake.variable("BOSUN_HALT", "1");
    sign(&fake, &k, H);
    two(&fake, H);
    two(&fake, H2);
    let (code, v) = go(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
    assert_eq!(v["data"]["approval_carried_from"], Value::Null, "{v}");
}

#[test]
fn a_carried_signature_never_clears_an_escalated_verdict() {
    // The carry passes with two verdicts on the new head, but a key is not the owner: an
    // escalated verdict on the new head stays escalated.
    let (fake, k) = world();
    fake.status(
        H2,
        "review-verdict",
        "failure",
        "escalated to the owner (approvable): approve 0.95 (approves at 0.85), 0 blocker, 0 major",
    );
    sign(&fake, &k, H);
    two(&fake, H);
    two(&fake, H2);
    let (code, v) = go(&fake);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["approval_carried_from"], H, "{v}");
    assert!(v["data"].get("verdict_cleared_by").is_none(), "{v}");
    // The escalated status stays on the head: nothing cleared it.
    assert_eq!(verdict_state(&fake, H2), "failure", "{v}");
}

/// The state of the newest `review-verdict` status on `sha`.
fn verdict_state(fake: &FakeGithub, sha: &str) -> Value {
    fake.statuses_of(sha)
        .into_iter()
        .rev()
        .find(|s| s["context"] == "review-verdict")
        .expect("a review-verdict status")["state"]
        .clone()
}
