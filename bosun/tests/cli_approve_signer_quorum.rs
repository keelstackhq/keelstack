//! `approve --as signer` under the quorum policy (D-022) and the autonomous policy (D-017): the
//! signer path, its refusals, and the refs the `[approvals]` policy is read from (the PR's base
//! and the default branch: both must allow it, fail closed).
mod common;

use bosun::github::b64;
use common::fake::FakeGithub;
use common::{bosun, lease_note, one_object, run};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};

const BASE: &str = "base0000";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const MAIN: &str = "refs/heads/main";
const NOW: &str = "2026-12-05T10:00:00Z";
const PLAIN: &str = "@@ -0,0 +1,1 @@\n+x";
const AS_SIGNER: [&str; 5] = ["--bootstrap", "--as", "signer", "--requester", "session_q"];

struct Key(Ed25519KeyPair, String);

fn key() -> Key {
    let rng = ring::rand::SystemRandom::new();
    let doc = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let label = concat!("PRIVATE", " KEY");
    let pem = format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        b64::encode(doc.as_ref())
    );
    Key(Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap(), pem)
}

fn config(policy: &str, signer: bool) -> String {
    let mut c = format!("project = \"widget\"\n\n[approvals]\npolicy = \"{policy}\"\n");
    if signer {
        c.push_str("signer = true\n");
    }
    c.push_str("\n[checks]\nmode = \"checks\"\napp_id = 4242\napp_slug = \"gate-app\"\n");
    c
}

fn checkout() -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap().keep();
    std::fs::create_dir_all(dir.join(".bosun")).unwrap();
    std::fs::write(dir.join(".bosun/bosun.toml"), config("quorum", true)).unwrap();
    dir
}

/// PR 1 by `octocat` at HEAD changing `src/a.rs` (closes task 7, leased by `session_w`), a green
/// gate-App `ci-ok`, the signer key enrolled, and one clean verdict per name in `reviewers`.
/// `base` and `main` are the `.bosun/bosun.toml` at the PR's base and at the default branch.
fn pr(k: &Key, base: &str, main: &str, reviewers: &[&str]) -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "Closes #7");
    fake.pull_as_issue(1, "change");
    fake.pull_set(1, "head", json!({"sha": HEAD, "ref": "feature"}));
    fake.pull_set(1, "user", json!({"login": "octocat"}));
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
    fake.content(
        ".bosun/approvers.toml",
        MAIN,
        &format!("[[key]]\nid = \"k1\"\ned25519 = \"{public}\"\nrole = \"signer\"\n"),
    );
    fake.content(".bosun/bosun.toml", BASE, base);
    fake.content(".bosun/bosun.toml", MAIN, main);
    fake.pull_file(1, "src/a.rs", "modified", 1, 0, Some(PLAIN));
    fake.app_run(HEAD, "ci-ok", "success", "ok", "gate-app", 4242);
    for session in reviewers {
        let r = json!({"bosun": "verdict", "session": session, "head_sha": HEAD,
            "approved": true, "confidence": 0.95, "major": 0});
        for bot in ["github-actions[bot]", "gate-app[bot]"] {
            fake.comment_by(1, bot, &format!("Review verdict: approve 0.95.\n\n{r}"));
        }
    }
    fake
}

fn sign(fake: &FakeGithub, k: &Key) -> (i32, Value) {
    let path = tempfile::tempdir().unwrap().keep().join("key.pem");
    std::fs::write(&path, &k.1).unwrap();
    let target = format!("1@{HEAD}");
    let dir = checkout();
    let mut args = vec!["-C", dir.to_str().unwrap(), "approve", &target];
    args.extend(["--sign-key", path.to_str().unwrap()]);
    args.extend(AS_SIGNER);
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_NOW", NOW)
        .env("BOSUN_SESSION", "signer-ci-1")
        .args(&args)
        .output()
        .unwrap();
    let v = one_object(&out.stdout);
    if v["ok"] == false {
        assert!(!v["error"]["fix"].as_str().unwrap().trim().is_empty());
    }
    (out.status.code().unwrap(), v)
}

fn refused(fake: &FakeGithub, k: &Key, what: &str) -> Value {
    let before = fake.comments_of(1).len();
    let (code, v) = sign(fake, k);
    assert_eq!(code, 1, "{what}: {v}");
    assert_eq!(fake.comments_of(1).len(), before, "{what}: nothing signed");
    v
}

#[test]
fn the_signer_signs_the_quorum_form_with_two_clean_verdicts() {
    let k = key();
    let cfg = config("quorum", true);
    let fake = pr(&k, &cfg, &cfg, &["session_r1", "session_r2"]);
    let (code, v) = sign(&fake, &k);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["approved"][0]["policy"], "quorum", "{v}");
    let (code, v) = run(&fake, "session_test", NOW, &["risk-check", "1"]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn the_quorum_signer_refuses_what_the_quorum_refuses() {
    let k = key();
    let cfg = config("quorum", true);
    // One verdict is not a quorum.
    let fake = pr(&k, &cfg, &cfg, &["session_r1"]);
    let v = refused(&fake, &k, "one verdict");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("quorum (D-022)"),
        "{v}"
    );
    // The requester and the task's lease holder are no reviewers.
    let fake = pr(&k, &cfg, &cfg, &["session_q", "session_w"]);
    refused(&fake, &k, "requester and author as reviewers");
    // ci-ok must be the gate App's green run.
    let fake = pr(&k, &cfg, &cfg, &["session_r1", "session_r2"]);
    fake.app_run(HEAD, "ci-ok", "failure", "bad", "gate-app", 4242);
    refused(&fake, &k, "red ci-ok");
    // The trust root is the owner's, even with two clean verdicts.
    let fake = pr(&k, &cfg, &cfg, &["session_r1", "session_r2"]);
    fake.pull_file(1, "governance/owners.toml", "modified", 1, 0, Some(PLAIN));
    let v = refused(&fake, &k, "trust root");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("trust root"),
        "{v}"
    );
    // A diff bosun cannot see (no patch) may hide the trust root.
    let fake = pr(&k, &cfg, &cfg, &["session_r1", "session_r2"]);
    fake.pull_file(1, "src/big.rs", "modified", 1, 0, None);
    refused(&fake, &k, "unseen diff");
    // The signer is opt-in.
    let off = config("quorum", false);
    let fake = pr(&k, &off, &off, &["session_r1", "session_r2"]);
    refused(&fake, &k, "signer off");
}

#[test]
fn the_policy_message_names_quorum_and_autonomous_not_only_autonomous() {
    let k = key();
    let cfg = config("owner-gated", true);
    let fake = pr(&k, &cfg, &cfg, &["session_r1", "session_r2"]);
    let v = refused(&fake, &k, "owner-gated");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("quorum (D-022) or autonomous (D-017)"),
        "{msg}"
    );
    assert!(!msg.contains("only under the autonomous policy"), "{msg}");
}

#[test]
fn signer_and_bootstrap_need_the_policy_at_both_the_base_and_the_default_branch() {
    let k = key();
    let (quorum, owner) = (config("quorum", true), config("owner-gated", true));
    // The default branch moved to quorum after this PR's base: nothing is signed.
    let fake = pr(&k, &owner, &quorum, &["session_r1", "session_r2"]);
    let v = refused(&fake, &k, "base owner-gated, main quorum");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("owner-gated"),
        "{v}"
    );
    // The default branch left quorum after the base: both refs must agree, so refused.
    let fake = pr(&k, &quorum, &owner, &["session_r1", "session_r2"]);
    refused(&fake, &k, "base quorum, main owner-gated");
}

#[test]
fn signer_off_at_either_ref_is_refused() {
    let k = key();
    let (on, off) = (config("quorum", true), config("quorum", false));
    let fake = pr(&k, &on, &off, &["session_r1", "session_r2"]);
    let v = refused(&fake, &k, "signer on at base, off on main");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("default branch"),
        "{v}"
    );
    let fake = pr(&k, &off, &on, &["session_r1", "session_r2"]);
    let v = refused(&fake, &k, "signer on at main, off at base");
    assert!(
        v["error"]["message"].as_str().unwrap().contains("base"),
        "{v}"
    );
}

const OLD: &str = "89abcdef0123456789abcdef0123456789abcdef";

/// Verdicts that name an older head are no verdicts on this one: exit 1, nothing signed.
#[test]
fn verdicts_on_an_old_head_do_not_count_for_the_signer() {
    let k = key();
    let cfg = config("quorum", true);
    let fake = pr(&k, &cfg, &cfg, &[]);
    for session in ["session_r1", "session_r2"] {
        let r = json!({"bosun": "verdict", "session": session, "head_sha": OLD,
            "approved": true, "confidence": 0.95, "major": 0});
        for bot in ["github-actions[bot]", "gate-app[bot]"] {
            fake.comment_by(1, bot, &format!("Review verdict: approve 0.95.\n\n{r}"));
        }
    }
    let v = refused(&fake, &k, "verdicts on an old head");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("quorum (D-022)"),
        "{v}"
    );
    // Control: the same two reviewers on the head sign.
    let fake = pr(&k, &cfg, &cfg, &["session_r1", "session_r2"]);
    assert_eq!(sign(&fake, &k).0, 0);
}

/// The session that asked (`--requester session_q`) is no reviewer: its verdict does not count.
#[test]
fn the_requesters_verdict_does_not_count_for_the_signer() {
    let k = key();
    let cfg = config("quorum", true);
    let fake = pr(&k, &cfg, &cfg, &["session_q", "session_r1"]);
    refused(&fake, &k, "requester plus one reviewer");
    // Control: a second independent reviewer makes the quorum.
    let fake = pr(&k, &cfg, &cfg, &["session_q", "session_r1", "session_r2"]);
    assert_eq!(sign(&fake, &k).0, 0);
}

/// The signer's own session (`BOSUN_SESSION`, `signer-ci-1` here) is no reviewer either.
#[test]
fn the_signers_own_verdict_does_not_count_for_the_signer() {
    let k = key();
    let cfg = config("quorum", true);
    let fake = pr(&k, &cfg, &cfg, &["signer-ci-1", "session_r1"]);
    refused(&fake, &k, "signer plus one reviewer");
    let fake = pr(&k, &cfg, &cfg, &["signer-ci-1", "session_r1", "session_r2"]);
    assert_eq!(sign(&fake, &k).0, 0);
}
