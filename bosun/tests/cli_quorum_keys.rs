//! D-022: under quorum a key signature of any role (owner v1/v2, autonomous, delegate) is a key,
//! never the owner. It counts only together with `QUORUM_VERDICTS` clean verdicts on the exact
//! head being judged; while bosun is halted no signature counts; and only the owner's own review
//! or `/bosun approve` comment clears an escalated verdict.
mod common;

use bosun::github::b64;
use common::fake::FakeGithub;
use common::{lease_note, run};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};

const BASE: &str = "base0000";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER: &str = "89abcdef0123456789abcdef0123456789abcdef";
const MAIN: &str = "refs/heads/main";
const NOW: &str = "2026-12-05T10:00:00Z";
const PLAIN: &str = "@@ -0,0 +1,1 @@\n+x";
const RULES: &str = concat!(
    include_str!("../src/core/risk_default.toml"),
    "\n[[rule]]\nid = \"R-feature\"\nwhy = \"x\"\npaths = [\"src/feature/**\"]\n",
    "\n[[rule]]\nid = \"R-deleg\"\nwhy = \"x\"\npaths = [\"src/deleg/**\"]\napproval = \"delegate\"\n"
);
const ESCALATED: &str =
    "escalated to the owner (approvable): approve 0.95 (approves at 0.85), 0 blocker, 0 major";

struct Key(Ed25519KeyPair);

fn key() -> Key {
    let rng = ring::rand::SystemRandom::new();
    let doc = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    Key(Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap())
}

#[derive(Clone, Copy, Debug)]
enum Sig {
    V1,
    V2,
    Autonomous,
    Delegate,
}

const ALL: [Sig; 4] = [Sig::V1, Sig::V2, Sig::Autonomous, Sig::Delegate];

/// PR 1 (Closes #7, leased by `session_w`) at HEAD changing `file` under base policy `quorum`.
/// `o1` is an owner key, `d1` a delegate key.
fn world(o: &Key, d: &Key, file: &str) -> FakeGithub {
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
    let po = b64::encode(o.0.public_key().as_ref());
    let pd = b64::encode(d.0.public_key().as_ref());
    let keys = format!(
        "[[key]]\nid = \"o1\"\ned25519 = \"{po}\"\nrole = \"owner\"\n\n[[key]]\nid = \"d1\"\ned25519 = \"{pd}\"\nrole = \"delegate\"\n"
    );
    fake.content(".bosun/approvers.toml", MAIN, &keys);
    let config = "project = \"widget\"\n\n[approvals]\npolicy = \"quorum\"\n";
    fake.content(".bosun/bosun.toml", BASE, config);
    fake.content(".bosun/bosun.toml", MAIN, config);
    fake.content(".bosun/risk.toml", BASE, RULES);
    fake.pull_file(1, file, "modified", 1, 0, Some(PLAIN));
    fake
}

fn file_for(s: Sig) -> &'static str {
    match s {
        Sig::Delegate => "src/deleg/a.rs",
        _ => "src/feature/a.rs",
    }
}

/// How `approved_by` names the key of each form: never the owner `ada`.
fn by_prefix(s: Sig) -> &'static str {
    match s {
        Sig::Delegate => "delegate d1",
        _ => "key o1",
    }
}

fn post(fake: &FakeGithub, o: &Key, d: &Key, s: Sig) {
    let (k, id, msg, first) = match s {
        Sig::V1 => (
            o,
            "o1",
            format!("bosun-approve v1 acme/widget#1 {HEAD}"),
            format!("/bosun approve {HEAD}"),
        ),
        Sig::V2 => (
            o,
            "o1",
            format!("bosun-approve v2 acme/widget#1 {HEAD} after-review 7"),
            format!("/bosun approve {HEAD} after-review 7"),
        ),
        Sig::Autonomous => (
            o,
            "o1",
            format!("bosun-autonomous v1 acme/widget#1 {HEAD}"),
            format!("/bosun approve {HEAD} autonomous"),
        ),
        Sig::Delegate => (
            d,
            "d1",
            format!("bosun-delegate v1 acme/widget#1 {HEAD}"),
            format!("/bosun approve {HEAD}"),
        ),
    };
    let sig = b64::encode(k.0.sign(msg.as_bytes()).as_ref());
    fake.comment_by(
        1,
        "claude",
        &format!("{first}\nbosun-sig ed25519 {id} {sig}"),
    );
    fake.last_comment(|c| c.via_app = Some("claude".into()));
}

fn verdict(fake: &FakeGithub, session: &str, head: &str) {
    let r = json!({"bosun": "verdict", "session": session, "head_sha": head,
        "approved": true, "confidence": 0.95, "major": 0});
    let body = format!("Review verdict: approve 0.95, 0 blocker.\n\n{r}");
    fake.comment_by(1, "github-actions[bot]", &body);
}

fn check(fake: &FakeGithub) -> (i32, Value) {
    run(fake, "session_test", NOW, &["risk-check", "1"])
}

/// Property: for every key form, on a PR off the trust root, the signature passes only with two
/// clean verdicts from two sessions on the judged head; none, one, two by one session, or two on
/// another head never pass.
#[test]
fn under_quorum_every_key_form_needs_two_verdicts_on_the_head() {
    let cases: [(&[(&str, &str)], bool); 5] = [
        (&[], false),
        (&[("session_r", HEAD)], false),
        (&[("session_r", HEAD), ("session_r", HEAD)], false),
        (&[("session_r", OTHER), ("session_s", OTHER)], false),
        (&[("session_r", HEAD), ("session_s", HEAD)], true),
    ];
    for s in ALL {
        for (verdicts, pass) in cases {
            let (o, d) = (key(), key());
            let fake = world(&o, &d, file_for(s));
            for (session, head) in verdicts {
                verdict(&fake, session, head);
            }
            post(&fake, &o, &d, s);
            let (code, v) = check(&fake);
            let what = format!("{s:?} {verdicts:?}");
            if pass {
                assert_eq!(code, 0, "{what}: {v}");
                assert_ne!(v["data"]["approved_by"], Value::Null, "{what}: {v}");
            } else {
                assert_eq!(code, 5, "{what}: {v}");
                assert_eq!(v["data"]["approved_by"], Value::Null, "{what}: {v}");
            }
        }
    }
}

#[test]
fn a_delegate_with_one_verdict_is_refused_under_quorum() {
    let (o, d) = (key(), key());
    let fake = world(&o, &d, file_for(Sig::Delegate));
    verdict(&fake, "session_r", HEAD);
    post(&fake, &o, &d, Sig::Delegate);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
}

#[test]
fn while_halted_no_key_signature_counts() {
    for s in ALL {
        let (o, d) = (key(), key());
        let fake = world(&o, &d, file_for(s));
        fake.variable("BOSUN_HALT", "1");
        verdict(&fake, "session_r", HEAD);
        verdict(&fake, "session_s", HEAD);
        post(&fake, &o, &d, s);
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{s:?}: {v}");
        assert_eq!(v["data"]["approved_by"], Value::Null, "{s:?}: {v}");
    }
}

#[test]
fn under_quorum_a_key_never_clears_an_escalated_verdict() {
    // High risk: the key passes with two verdicts, but it is not the owner's own approval.
    for s in ALL {
        let (o, d) = (key(), key());
        let fake = world(&o, &d, file_for(s));
        fake.status(HEAD, "review-verdict", "failure", ESCALATED);
        verdict(&fake, "session_r", HEAD);
        verdict(&fake, "session_s", HEAD);
        post(&fake, &o, &d, s);
        let (code, v) = check(&fake);
        assert_eq!(code, 0, "{s:?}: {v}");
        let by = v["data"]["approved_by"].as_str().unwrap_or_default();
        assert!(by.starts_with(by_prefix(s)), "{s:?}: {v}");
        assert!(v["data"].get("verdict_cleared_by").is_none(), "{s:?}: {v}");
    }
    // Low risk: a key is no owner approval either.
    for s in ALL {
        let (o, d) = (key(), key());
        let fake = world(&o, &d, "notes/a.txt");
        fake.status(HEAD, "review-verdict", "failure", ESCALATED);
        verdict(&fake, "session_r", HEAD);
        verdict(&fake, "session_s", HEAD);
        post(&fake, &o, &d, s);
        let (code, v) = check(&fake);
        assert_eq!(code, 0, "{s:?}: {v}");
        assert_eq!(v["data"]["level"], "normal", "{s:?}: {v}");
        // Normal level needs no approval: the key is not recorded as the owner either.
        assert_ne!(v["data"]["approved_by"], "ada", "{s:?}: {v}");
        assert!(v["data"].get("verdict_cleared_by").is_none(), "{s:?}: {v}");
    }
    // The owner's own comment still clears it.
    let (o, d) = (key(), key());
    let fake = world(&o, &d, "notes/a.txt");
    fake.status(HEAD, "review-verdict", "failure", ESCALATED);
    fake.comment_by(1, "ada", &format!("/bosun approve {HEAD}"));
    let (code, v) = check(&fake);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["verdict_cleared_by"], "ada", "{v}");
}
