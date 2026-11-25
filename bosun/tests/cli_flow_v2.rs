//! Flow v2 (D-028): `[review] verdicts_needed` and `[review] block_on` change how many counting
//! verdicts a quorum approval needs and which findings keep a verdict from counting. The defaults
//! (two verdicts, a major blocks) are pinned by cli_quorum.rs.
mod common;

use bosun::config::Config;
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
const RULES: &str = concat!(
    include_str!("../src/core/risk_default.toml"),
    "\n[[rule]]\nid = \"R-feature\"\nwhy = \"x\"\npaths = [\"src/feature/**\"]\n"
);

struct Key(Ed25519KeyPair);

fn config(review: &str) -> String {
    format!("project = \"widget\"\n\n[approvals]\npolicy = \"quorum\"\n\n[review]\n{review}\n")
}

fn verdict_of(session: &str, major: u32) -> String {
    let r = json!({"bosun": "verdict", "session": session, "head_sha": HEAD,
        "approved": true, "confidence": 0.95, "major": major});
    format!("Review verdict: approve 0.95, 0 blocker.\n\n{r}")
}

fn pr(review: &str) -> (FakeGithub, Key) {
    let rng = ring::rand::SystemRandom::new();
    let doc = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let k = Key(Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap());
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
    fake.content(".bosun/bosun.toml", BASE, &config(review));
    fake.content(".bosun/risk.toml", BASE, RULES);
    fake.content(".bosun/bosun.toml", MAIN, &config(review));
    fake.pull_file(1, "src/feature/a.rs", "modified", 1, 0, Some(PLAIN));
    (fake, k)
}

/// Verdicts of `(session, major findings)`, then the signature, then risk-check.
fn with(fake: &FakeGithub, k: &Key, verdicts: &[(&str, u32)]) -> (i32, Value) {
    for (s, major) in verdicts {
        fake.comment_by(1, "github-actions[bot]", &verdict_of(s, *major));
    }
    let msg = format!("bosun-autonomous v1 acme/widget#1 {HEAD}");
    let s = b64::encode(k.0.sign(msg.as_bytes()).as_ref());
    let body = format!("/bosun approve {HEAD} autonomous\nbosun-sig ed25519 k1 {s}");
    fake.comment_by(1, "claude", &body);
    fake.last_comment(|c| c.via_app = Some("claude".into()));
    run(fake, "session_test", NOW, &["risk-check", "1"])
}

fn description(fake: &FakeGithub) -> String {
    let s = fake.statuses_of(HEAD);
    let s = s
        .iter()
        .rev()
        .find(|s| s["context"] == "risk-check")
        .unwrap();
    s["description"].as_str().unwrap().to_string()
}

#[test]
fn one_verdict_is_enough_when_verdicts_needed_is_one() {
    let (fake, k) = pr("verdicts_needed = 1");
    let (code, v) = with(&fake, &k, &[("session_r", 0)]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["quorum_verdicts"], json!(["session_r"]));
}

#[test]
fn one_verdict_needed_still_excludes_the_authors_session_and_dedupes() {
    let (fake, k) = pr("verdicts_needed = 1");
    let (code, v) = with(&fake, &k, &[("session_w", 0)]);
    assert_eq!(code, 5, "{v}");
    let d = description(&fake);
    assert!(d.contains("1 clean verdict and orchestrator"), "{d}");

    let (fake, k) = pr("verdicts_needed = 3");
    let (code, v) = with(&fake, &k, &[("a", 0), ("a", 0), ("b", 0)]);
    assert_eq!(code, 5, "{v}");
    let d = description(&fake);
    assert!(d.contains("3 clean verdicts"), "{d}");
}

#[test]
fn a_major_finding_does_not_block_when_block_on_is_blocker() {
    let (fake, k) = pr("verdicts_needed = 1\nblock_on = \"blocker\"");
    let (code, v) = with(&fake, &k, &[("session_r", 2)]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_major_finding_blocks_by_default_even_with_one_verdict_needed() {
    let (fake, k) = pr("verdicts_needed = 1");
    let (code, v) = with(&fake, &k, &[("session_r", 1)]);
    assert_eq!(code, 5, "{v}");
}

#[test]
fn a_rejection_still_blocks_when_block_on_is_blocker() {
    let (fake, k) = pr("verdicts_needed = 1\nblock_on = \"blocker\"");
    let r = json!({"bosun": "verdict", "session": "session_x", "head_sha": HEAD,
        "approved": false, "confidence": 0.95, "major": 0});
    fake.comment_by(
        1,
        "github-actions[bot]",
        &format!("Review verdict: request-changes.\n\n{r}"),
    );
    let (code, v) = with(&fake, &k, &[("session_r", 0)]);
    assert_eq!(code, 5, "{v}");
}

#[test]
fn config_parses_the_review_flow_and_rejects_bad_values() {
    let c = Config::parse(
        "[review]\nverdicts_needed = 1\nblock_on = \"blocker\"\n",
        "f",
    )
    .unwrap();
    assert_eq!(c.review.verdicts_needed, 1);
    let d = Config::parse("", "f").unwrap();
    assert_eq!(d.review.verdicts_needed, 2);
    assert!(Config::parse("[review]\nverdicts_needed = 0\n", "f").is_err());
    assert!(Config::parse("[review]\nblock_on = \"minor\"\n", "f").is_err());
    assert!(Config::parse("[main_health]\nblock = false\n", "f").is_ok());
    assert!(Config::parse("[main_health]\nwarn = false\n", "f").is_err());
}
