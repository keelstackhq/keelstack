//! `[approvals] owner_delegation = "all"` (D-027): under the quorum policy the owner delegates
//! every PR, the trust root and R-weaken included, to an enrolled key's quorum signature. The
//! floor stays: two counting verdicts on the exact head, a linked leased task, no halt, no
//! unseen diff, no strong secret. Read at the PR's base only; off, nothing changes.
mod common;

use bosun::github::b64;
use common::fake::FakeGithub;
use common::{bosun, lease_note, one_object, run};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};

const BASE: &str = "base0000";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER: &str = "89abcdef0123456789abcdef0123456789abcdef";
const MAIN: &str = "refs/heads/main";
const NOW: &str = "2026-12-05T10:00:00Z";
const PLAIN: &str = "@@ -0,0 +1,1 @@\n+x";
const WAITING: &str = "high risk, owner approval needed";
const ON: &str = "owner_delegation = \"all\"\n";

struct Key(Ed25519KeyPair, String);

fn key() -> Key {
    let rng = ring::rand::SystemRandom::new();
    let doc = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let pem = format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        b64::encode(doc.as_ref())
    );
    Key(Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap(), pem)
}

fn config(extra: &str) -> String {
    format!("project = \"widget\"\n\n[approvals]\npolicy = \"quorum\"\n{extra}")
}

fn verdict_of(session: &str, head: &str) -> String {
    let r = json!({"bosun": "verdict", "session": session, "head_sha": head,
        "approved": true, "confidence": 0.95, "major": 0});
    format!("Review verdict: approve 0.95, 0 blocker.\n\n{r}")
}

/// PR 1 (Closes #7, leased by `session_w`) at HEAD changing `file`; `base` and `main` are the
/// extra `[approvals]` lines at the PR's base and on the default branch.
fn pr(k: &Key, file: &str, base: &str, main: &str) -> FakeGithub {
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
    fake.content(".bosun/bosun.toml", BASE, &config(base));
    fake.content(".bosun/bosun.toml", MAIN, &config(main));
    fake.content(
        ".bosun/risk.toml",
        BASE,
        include_str!("../src/core/risk_default.toml"),
    );
    fake.pull_file(1, file, "modified", 1, 0, Some(PLAIN));
    fake
}

fn on(k: &Key, file: &str) -> FakeGithub {
    pr(k, file, ON, ON)
}

fn post_sig(fake: &FakeGithub, k: &Key) {
    let msg = format!("bosun-autonomous v1 acme/widget#1 {HEAD}");
    let s = b64::encode(k.0.sign(msg.as_bytes()).as_ref());
    let body = format!("/bosun approve {HEAD} autonomous\nbosun-sig ed25519 k1 {s}");
    fake.comment_by(1, "claude", &body);
    fake.last_comment(|c| c.via_app = Some("claude".into()));
}

fn post_verdicts(fake: &FakeGithub, sessions: &[&str], head: &str) {
    for s in sessions {
        fake.comment_by(1, "github-actions[bot]", &verdict_of(s, head));
    }
}

fn check(fake: &FakeGithub) -> (i32, Value) {
    run(fake, "session_test", NOW, &["risk-check", "1"])
}

fn description(fake: &FakeGithub) -> String {
    let s = fake.statuses_of(HEAD);
    let s = (s.iter().rev())
        .find(|s| s["context"] == "risk-check")
        .unwrap();
    s["description"].as_str().unwrap().to_string()
}

fn sign(fake: &FakeGithub, k: &Key) -> (i32, Value) {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("key.pem");
    std::fs::write(&path, &k.1).unwrap();
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_NOW", NOW)
        .env("BOSUN_SESSION", "session_o")
        .env("CCR_AGENT_PROXY_ENABLED", "true")
        .args(["approve", &format!("1@{HEAD}"), "--bootstrap", "--print"])
        .arg("--sign-key")
        .arg(&path)
        .output()
        .unwrap();
    let v = one_object(&out.stdout);
    if v["ok"] == false {
        assert!(!v["error"]["fix"].as_str().unwrap().trim().is_empty());
    }
    (out.status.code().unwrap(), v)
}

const TRUST_ROOT: [&str; 4] = [
    ".bosun/approvers.toml",
    ".bosun/bosun.toml",
    ".github/workflows/ci.yml",
    "decisions/DECISIONS.md",
];

#[test]
fn on_the_quorum_signature_approves_the_trust_root_and_a_weakening() {
    for file in TRUST_ROOT {
        let k = key();
        let fake = on(&k, file);
        fake.label(1, "bosun:capability/weaken");
        post_verdicts(&fake, &["session_r", "session_s"], HEAD);
        let (code, v) = sign(&fake, &k);
        assert_eq!(code, 0, "{file}: {v}");
        let body = v["data"]["body"].as_str().unwrap();
        assert!(body.contains("owner delegation (D-027)"), "{file}: {body}");
        post_sig(&fake, &k);
        let (code, v) = check(&fake);
        assert_eq!(code, 0, "{file}: {v}");
        assert!(
            v["data"]["approved_by"]
                .as_str()
                .unwrap()
                .starts_with("key k1 (quorum with owner delegation"),
            "{file}: {v}"
        );
        assert_eq!(v["data"]["owner_delegation"], true, "{file}: {v}");
        assert!(description(&fake).contains("D-027"), "{file}");
    }
}

#[test]
fn off_the_trust_root_stays_with_the_owner() {
    for file in TRUST_ROOT {
        let k = key();
        let fake = pr(&k, file, "", "");
        post_verdicts(&fake, &["session_r", "session_s"], HEAD);
        assert_eq!(sign(&fake, &k).0, 1, "{file}");
        post_sig(&fake, &k);
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{file}: {v}");
        assert!(description(&fake).starts_with(WAITING), "{file}");
        assert_eq!(v["data"]["owner_delegation"], false, "{file}: {v}");
    }
}

#[test]
fn on_the_floor_stays_verdicts_on_the_exact_head() {
    let k = key();
    // One verdict only.
    let fake = on(&k, ".bosun/approvers.toml");
    post_verdicts(&fake, &["session_r"], HEAD);
    assert_eq!(sign(&fake, &k).0, 1);
    post_sig(&fake, &k);
    assert_eq!(check(&fake).0, 5);
    // No verdict at all: the signature alone never passes.
    let fake = on(&k, ".bosun/approvers.toml");
    post_sig(&fake, &k);
    assert_eq!(check(&fake).0, 5);
    // Two verdicts on a stale head.
    let fake = on(&k, ".bosun/approvers.toml");
    post_verdicts(&fake, &["session_r", "session_s"], OTHER);
    assert_eq!(sign(&fake, &k).0, 1);
    post_sig(&fake, &k);
    assert_eq!(check(&fake).0, 5);
    // Two verdicts from one session count once.
    let fake = on(&k, ".bosun/approvers.toml");
    post_verdicts(&fake, &["session_r", "session_r"], HEAD);
    post_sig(&fake, &k);
    assert_eq!(check(&fake).0, 5);
}

#[test]
fn on_a_halt_a_strong_secret_and_an_unlinked_pr_still_count_no_signature() {
    let k = key();
    let fake = on(&k, ".bosun/approvers.toml");
    fake.variable("BOSUN_HALT", "1");
    post_verdicts(&fake, &["session_r", "session_s"], HEAD);
    post_sig(&fake, &k);
    assert_eq!(check(&fake).0, 5);
    assert_eq!(sign(&fake, &k).0, 4);
    let fake = on(&k, ".bosun/approvers.toml");
    let pem = "@@ -0,0 +1,1 @@\n+-----BEGIN PRIVATE KEY-----";
    fake.pull_file(1, "docs/notes.txt", "added", 1, 0, Some(pem));
    post_verdicts(&fake, &["session_r", "session_s"], HEAD);
    assert_eq!(sign(&fake, &k).0, 1);
    post_sig(&fake, &k);
    assert_eq!(check(&fake).0, 5);
    assert!(description(&fake).starts_with(WAITING));
    let fake = on(&k, ".bosun/approvers.toml");
    fake.pull_set(1, "body", json!("no task"));
    post_verdicts(&fake, &["session_r", "session_s"], HEAD);
    post_sig(&fake, &k);
    assert_eq!(check(&fake).0, 5);
}

#[test]
fn the_setting_is_read_from_the_base_never_the_head() {
    let k = key();
    // The PR turns delegation on for itself: base and main are off, so the owner approves.
    let fake = pr(&k, ".bosun/bosun.toml", "", "");
    fake.content(".bosun/bosun.toml", HEAD, &config(ON));
    post_verdicts(&fake, &["session_r", "session_s"], HEAD);
    assert_eq!(sign(&fake, &k).0, 1);
    post_sig(&fake, &k);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(WAITING));
}

#[test]
fn approve_fails_closed_when_main_turned_delegation_off() {
    let k = key();
    // The base has it, the default branch no longer does: the stricter default wins.
    let fake = pr(&k, ".bosun/approvers.toml", ON, "");
    post_verdicts(&fake, &["session_r", "session_s"], HEAD);
    let (code, v) = sign(&fake, &k);
    assert_eq!(code, 1, "{v}");
}

#[test]
fn the_setting_needs_the_quorum_policy_and_the_value_all() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".bosun")).unwrap();
    for (text, ok) in [
        (config(ON), true),
        (config("owner_delegation = \"some\"\n"), false),
        (
            "project = \"widget\"\n\n[approvals]\npolicy = \"autonomous\"\nowner_delegation = \"all\"\n".to_string(),
            false,
        ),
        (
            "project = \"widget\"\n\n[approvals]\nowner_delegation = \"all\"\n".to_string(),
            false,
        ),
    ] {
        let r = bosun::config::Config::parse(&text, ".bosun/bosun.toml");
        assert_eq!(r.is_ok(), ok, "{text}");
    }
}

fn sign_plain(fake: &FakeGithub, k: &Key) -> (i32, Value) {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("key.pem");
    std::fs::write(&path, &k.1).unwrap();
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_NOW", NOW)
        .env("BOSUN_SESSION", "session_o")
        .env("CCR_AGENT_PROXY_ENABLED", "true")
        .args(["approve", &format!("1@{HEAD}"), "--print"])
        .arg("--sign-key")
        .arg(&path)
        .output()
        .unwrap();
    let v = one_object(&out.stdout);
    (out.status.code().unwrap(), v)
}

#[test]
fn on_a_plain_signature_never_covers_the_trust_root() {
    let k = key();
    let fake = on(&k, ".bosun/approvers.toml");
    post_verdicts(&fake, &["session_r", "session_s"], HEAD);
    let (code, v) = sign_plain(&fake, &k);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["message"].as_str().unwrap().contains("D-027"),
        "{v}"
    );
}

#[test]
fn a_base_config_that_does_not_parse_fails_closed_whatever_main_says() {
    let k = key();
    let fake = on(&k, ".bosun/approvers.toml");
    fake.content(
        ".bosun/bosun.toml",
        BASE,
        "project = \"widget\"\n[approvals]\nbogus = 1\n",
    );
    post_verdicts(&fake, &["session_r", "session_s"], HEAD);
    assert_ne!(sign(&fake, &k).0, 0);
    post_sig(&fake, &k);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["owner_delegation"], false, "{v}");
}

/// A plain owner-role signature comment (v1, or v2 after `review`) by `k1` on HEAD.
fn post_plain_sig(fake: &FakeGithub, k: &Key, review: Option<u64>) {
    let (msg, first) = match review {
        None => (
            format!("bosun-approve v1 acme/widget#1 {HEAD}"),
            format!("/bosun approve {HEAD}"),
        ),
        Some(id) => (
            format!("bosun-approve v2 acme/widget#1 {HEAD} after-review {id}"),
            format!("/bosun approve {HEAD} after-review {id}"),
        ),
    };
    let s = b64::encode(k.0.sign(msg.as_bytes()).as_ref());
    fake.comment_by(1, "claude", &format!("{first}\nbosun-sig ed25519 k1 {s}"));
    fake.last_comment(|c| c.via_app = Some("claude".into()));
}

#[test]
fn on_risk_check_counts_no_plain_signature_on_the_trust_root() {
    for file in TRUST_ROOT {
        // v1 with two clean verdicts.
        let k = key();
        let fake = on(&k, file);
        post_verdicts(&fake, &["session_r", "session_s"], HEAD);
        post_plain_sig(&fake, &k, None);
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{file} v1: {v}");
        assert!(v["data"]["approved_by"].is_null(), "{file} v1: {v}");
        assert!(
            description(&fake).contains("orchestrator signature needed"),
            "{file} v1"
        );
        // The quorum form still approves next to the refused plain one.
        post_sig(&fake, &k);
        let (code, v) = check(&fake);
        assert_eq!(code, 0, "{file} v1 + quorum: {v}");
        assert!(
            (v["data"]["approved_by"].as_str().unwrap())
                .starts_with("key k1 (quorum with owner delegation"),
            "{file}: {v}"
        );
        // v2 bound to a dismissed owner review, with two clean verdicts.
        let k = key();
        let fake = on(&k, file);
        fake.review(1, "ada", "CHANGES_REQUESTED", HEAD);
        fake.review(1, "ada", "DISMISSED", HEAD);
        fake.dismiss_event(1, 2, "2026-10-22T10:30:00Z");
        post_verdicts(&fake, &["session_r", "session_s"], HEAD);
        post_plain_sig(&fake, &k, Some(2));
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{file} v2: {v}");
        assert!(v["data"]["approved_by"].is_null(), "{file} v2: {v}");
    }
}
