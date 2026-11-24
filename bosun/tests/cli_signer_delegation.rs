//! The CI signer (`approve --as signer`) under the quorum policy with owner delegation (D-027):
//! "the quorum signature (an enrolled key's, or the CI signer's) also approves the trust root and
//! R-weaken", with the quorum floor unchanged. Without delegation the trust root stays the
//! owner's.
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

/// This repo's shape: quorum, the signer on, the gate App in checks mode; `delegation` adds
/// `owner_delegation = "all"`.
fn config(delegation: bool) -> String {
    let mut c = String::from("project = \"widget\"\n\n[approvals]\npolicy = \"quorum\"\n");
    if delegation {
        c.push_str("owner_delegation = \"all\"\n");
    }
    c.push_str("signer = true\n");
    c.push_str("\n[checks]\nmode = \"checks\"\napp_id = 4242\napp_slug = \"gate-app\"\n");
    c
}

fn checkout(cfg: &str) -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap().keep();
    std::fs::create_dir_all(dir.join(".bosun")).unwrap();
    std::fs::write(dir.join(".bosun/bosun.toml"), cfg).unwrap();
    dir
}

/// PR 1 at HEAD changing `file` (closes task 7, leased by `session_w`), a green gate-App
/// `ci-ok`, the signer key enrolled and two clean verdicts from different reviewer sessions.
fn pr(k: &Key, cfg: &str, file: &str) -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "Closes #7");
    fake.pull_as_issue(1, "change");
    fake.pull_set(1, "head", json!({"sha": HEAD, "ref": "feature"}));
    // ci.yml on the default branch and its green run of this PR: the signer re-reads it.
    fake.content(
        ".github/workflows/ci.yml",
        "refs/heads/main",
        "jobs:\n  ci-ok:\n    runs-on: x\n",
    );
    fake.workflow_file_run(
        HEAD,
        ".github/workflows/ci.yml",
        "pull_request",
        "completed",
        Some("success"),
    );
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
        &format!("[[key]]\nid = \"signer-ci\"\ned25519 = \"{public}\"\nrole = \"signer\"\n"),
    );
    fake.content(".bosun/bosun.toml", BASE, cfg);
    fake.content(".bosun/bosun.toml", MAIN, cfg);
    fake.content(
        ".bosun/risk.toml",
        BASE,
        include_str!("../src/core/risk_default.toml"),
    );
    fake.pull_file(1, file, "modified", 1, 0, Some(PLAIN));
    fake.app_run(HEAD, "bosun-ci-ok", "success", "ok", "gate-app", 4242);
    for session in ["session_r1", "session_r2"] {
        let r = json!({"bosun": "verdict", "session": session, "head_sha": HEAD,
            "approved": true, "confidence": 0.95, "major": 0});
        for bot in ["github-actions[bot]", "gate-app[bot]"] {
            fake.comment_by(1, bot, &format!("Review verdict: approve 0.95.\n\n{r}"));
        }
    }
    fake
}

fn sign(fake: &FakeGithub, k: &Key, cfg: &str) -> (i32, Value) {
    let path = tempfile::tempdir().unwrap().keep().join("key.pem");
    std::fs::write(&path, &k.1).unwrap();
    let target = format!("1@{HEAD}");
    let dir = checkout(cfg);
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

/// Trust-root files the signer may sign under delegation. Anything ci.yml executes from the
/// checkout (`.github/**`, local actions, R-ci paths) is refused instead:
/// `the_signer_refuses_a_pr_that_changes_what_ci_yml_runs`.
const TRUST_ROOT: [&str; 3] = [
    ".bosun/approvers.toml",
    ".bosun/bosun.toml",
    "decisions/DECISIONS.md",
];

#[test]
fn with_delegation_the_signer_approves_the_trust_root() {
    let cfg = config(true);
    for file in TRUST_ROOT {
        let k = key();
        let fake = pr(&k, &cfg, file);
        let (code, v) = sign(&fake, &k, &cfg);
        assert_eq!(code, 0, "{file}: {v}");
        assert_eq!(v["data"]["approved"][0]["policy"], "quorum", "{file}: {v}");
        let (code, v) = run(&fake, "session_test", NOW, &["risk-check", "1"]);
        assert_eq!(code, 0, "{file}: {v}");
        assert_eq!(v["data"]["owner_delegation"], true, "{file}: {v}");
    }
}

#[test]
fn with_delegation_the_signer_approves_a_trust_root_weakening() {
    let cfg = config(true);
    let k = key();
    let fake = pr(&k, &cfg, ".bosun/risk.toml");
    fake.label(1, "bosun:capability/weaken");
    let (code, v) = sign(&fake, &k, &cfg);
    assert_eq!(code, 0, "{v}");
    let (code, v) = run(&fake, "session_test", NOW, &["risk-check", "1"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["owner_delegation"], true, "{v}");
}

#[test]
fn without_delegation_the_trust_root_stays_the_owners() {
    let cfg = config(false);
    for file in TRUST_ROOT {
        let k = key();
        let fake = pr(&k, &cfg, file);
        let (code, v) = sign(&fake, &k, &cfg);
        assert_eq!(code, 1, "{file}: {v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("trust root"),
            "{file}: {v}"
        );
        assert_eq!(fake.comments_of(1).len(), 4, "{file}: nothing signed");
    }
}

#[test]
fn with_delegation_the_signer_still_needs_two_verdicts_and_a_green_gate_ci_ok() {
    let cfg = config(true);
    let k = key();
    let fake = pr(&k, &cfg, ".bosun/approvers.toml");
    fake.app_run(HEAD, "bosun-ci-ok", "failure", "bad", "gate-app", 4242);
    let (code, v) = sign(&fake, &k, &cfg);
    assert_eq!(code, 1, "red ci-ok: {v}");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(msg.contains("gate App"), "{msg}");
    assert!(!msg.contains("GitHub Actions check run counts"), "{msg}");
}

#[test]
fn the_signer_refuses_a_pr_that_changes_what_ci_yml_runs() {
    // The mirrored ci-ok comes from the PR's own copy of ci.yml (a pull_request run): a PR that
    // changes it could make its own ci-ok green, so the keyless signer never signs it.
    let cfg = config(true);
    for (file, prev) in [
        (".github/workflows/ci.yml", None),
        (".github/workflows/x.yml", Some(".github/workflows/ci.yml")),
        (".github/scripts/affected.sh", None),
        ("scripts/bump-version.sh", None),
        ("rust-toolchain.toml", None),
    ] {
        let k = key();
        let fake = pr(&k, &cfg, file);
        if let Some(p) = prev {
            fake.renamed_from(1, p);
        }
        let (code, v) = sign(&fake, &k, &cfg);
        assert_eq!(code, 1, "{file}: {v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("executes from the checkout"),
            "{file}: {v}"
        );
        assert_eq!(fake.comments_of(1).len(), 4, "{file}: nothing signed");
    }
}

#[test]
fn the_signer_refuses_while_the_ruleset_does_not_pin_the_gates() {
    let cfg = config(true);
    let k = key();
    let fake = pr(&k, &cfg, "src/a.rs");
    fake.unpin_gates();
    let (code, v) = sign(&fake, &k, &cfg);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["fix"]
            .as_str()
            .unwrap()
            .contains("hoist apply --area github"),
        "{v}"
    );
    assert_eq!(fake.comments_of(1).len(), 4, "nothing signed");
}

#[test]
fn a_stale_green_bosun_ci_ok_never_counts_after_ci_yml_turns_red() {
    // The mirror posted green earlier; ci.yml was re-run since and its ci-ok failed.
    let cfg = config(true);
    let k = key();
    let fake = pr(&k, &cfg, "src/a.rs");
    fake.workflow_file_run(
        HEAD,
        ".github/workflows/ci.yml",
        "pull_request",
        "completed",
        Some("failure"),
    );
    let (code, v) = sign(&fake, &k, &cfg);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not green now"),
        "{v}"
    );
    assert_eq!(fake.comments_of(1).len(), 4, "nothing signed");
}
