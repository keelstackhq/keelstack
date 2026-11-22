//! `[approvals] policy = "quorum"` (D-022): an autonomous-form signature plus two counting
//! verdicts from different sessions passes on every path except the trust root, and
//! `bosun approve --bootstrap` signs only under the same conditions.
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
const GATE: &str = "src/feature/a.rs";
const RULES: &str = concat!(
    include_str!("../src/core/risk_default.toml"),
    "\n[[rule]]\nid = \"R-feature\"\nwhy = \"x\"\npaths = [\"src/feature/**\"]\n"
);
const WAITING: &str = "high risk, owner approval needed";
const HALTED: &str = "high risk, quorum (D-022): bosun is halted, no signature counts:";
const UNREADABLE: &str =
    "high risk, quorum (D-022): cannot tell whether bosun is halted, no signature counts:";
const QUORUM: &str =
    "high risk, quorum (D-022): 2 clean verdicts and orchestrator signature needed";

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

fn config(policy: &str) -> String {
    format!("project = \"widget\"\n\n[approvals]\npolicy = \"{policy}\"\n")
}

/// [`config`] with `[checks] mode = "checks"` (the gate App): it does not make a workflow change
/// less of a trust root, see `a_workflow_change_is_the_owners_in_every_checks_mode`.
fn config_checks(policy: &str) -> String {
    format!(
        "{}\n[checks]\nmode = \"checks\"\napp_id = 1\napp_slug = \"gate\"\n",
        config(policy)
    )
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

fn description(fake: &FakeGithub) -> String {
    let s = fake.statuses_of(HEAD);
    let s = s
        .iter()
        .rev()
        .find(|s| s["context"] == "risk-check")
        .unwrap();
    s["description"].as_str().unwrap().to_string()
}

/// Signature plus the verdicts of `sessions` on `fake`.
fn with(fake: &FakeGithub, k: &Key, sessions: &[&str]) -> (i32, Value) {
    for s in sessions {
        post_verdict(fake, s, HEAD);
    }
    post_sig(fake, k);
    check(fake)
}

fn owner_path(fake: &FakeGithub, v: &Value, what: &str) {
    assert!(description(fake).starts_with(WAITING), "{what}: {v}");
}

#[test]
fn two_verdicts_pass_a_critical_floor_pr_with_hoist_toml_and_weaken() {
    let k = key();
    let fake = quorum(&k);
    fake.content(".bosun/bosun.toml", BASE, &config_checks("quorum"));
    // The critical floor (`hoist.toml`, `src/**`) and the weaken capability (D-022 approves a
    // weakening too: it superseded D-017's R-weaken exception). A workflow is trust root now, in
    // every mode (`a_workflow_change_is_the_owners_in_every_checks_mode`).
    fake.pull_file(1, "hoist.toml", "modified", 1, 0, Some(PLAIN));
    fake.pull_file(1, "src/core/meta.rs", "modified", 1, 0, Some(PLAIN));
    fake.label(1, "bosun:capability/weaken");
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 0, "{v}");
    let by = v["data"]["approved_by"].as_str().unwrap();
    assert!(
        by.starts_with("key k1 (") && by.contains("quorum, D-022"),
        "{by}"
    );
    assert_eq!(v["data"]["policy"], "quorum");
    assert_eq!(v["data"]["policy_source"], "explicit");
    assert_eq!(
        v["data"]["quorum_verdicts"],
        json!(["session_r", "session_s"])
    );
}

#[test]
fn one_verdict_or_none_does_not_pass_and_the_wait_is_not_an_owner_wait() {
    for sessions in [&["session_r"][..], &[][..]] {
        let k = key();
        let fake = quorum(&k);
        let (code, v) = with(&fake, &k, sessions);
        assert_eq!(code, 5, "{v}");
        let d = description(&fake);
        assert!(d.starts_with(QUORUM) && !d.starts_with(WAITING), "{d}");
        assert_eq!(v["data"]["policy"], "quorum");
    }
}

#[test]
fn two_verdicts_without_a_signature_do_not_pass() {
    let k = key();
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    // No signature: the verdicts alone do not approve a high-risk PR of this class.
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
}

#[test]
fn two_verdicts_from_the_same_session_count_once() {
    let k = key();
    let fake = quorum(&k);
    let (code, v) = with(&fake, &k, &["session_r", "session_r"]);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(QUORUM));
}

#[test]
fn a_verdict_from_the_author_or_a_lease_holder_does_not_count() {
    let k = key();
    let fake = quorum(&k);
    let (code, v) = with(&fake, &k, &["session_w", "session_r"]);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(QUORUM));
    // A leaseholder of another linked issue is excluded as well.
    let fake = quorum(&k);
    fake.issue(8, "other", &[], "x");
    fake.pull_set(1, "body", json!("Closes #7\nCloses #8"));
    fake.comment(
        8,
        &lease_note("session_s", "2026-12-06T12:00:00Z", "src/**"),
    );
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
}

#[test]
fn a_request_for_changes_on_the_head_blocks_even_with_two_clean_verdicts() {
    for before in [true, false] {
        let k = key();
        let fake = quorum(&k);
        let reject = |f: &FakeGithub| {
            f.comment_by(
                1,
                "github-actions[bot]",
                &verdict_of("session_x", HEAD, 1, false),
            )
        };
        if before {
            reject(&fake);
        }
        post_verdict(&fake, "session_r", HEAD);
        post_verdict(&fake, "session_s", HEAD);
        if !before {
            reject(&fake);
        }
        post_sig(&fake, &k);
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{before}: {v}");
        assert!(description(&fake).starts_with(QUORUM), "{before}");
        assert_eq!(sign(&fake, &k, "session_o").0, 1, "{before}");
    }
}

#[test]
fn verdicts_on_another_head_or_with_a_major_finding_do_not_count() {
    let k = key();
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", OTHER);
    post_sig(&fake, &k);
    assert_eq!(check(&fake).0, 5);
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    fake.comment_by(
        1,
        "github-actions[bot]",
        &verdict_of("session_s", HEAD, 1, true),
    );
    post_sig(&fake, &k);
    assert_eq!(check(&fake).0, 5);
}

#[test]
fn the_trust_root_stays_with_the_owner_also_by_rename_and_case() {
    for (file, from) in [
        (".bosun/approvers.toml", None),
        (".bosun/bosun.toml", None),
        (".BOSUN/Approvers.toml", None),
        ("docs/moved.txt", Some(".bosun/approvers.toml")),
        ("docs/moved.txt", Some(".bosun/bosun.toml")),
        ("src/../x.rs", None),
    ] {
        let k = key();
        let fake = quorum(&k);
        let status = if from.is_some() {
            "renamed"
        } else {
            "modified"
        };
        fake.pull_file(1, file, status, 1, 0, Some(PLAIN));
        if let Some(f) = from {
            fake.renamed_from(1, f);
        }
        let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
        assert_eq!(code, 5, "{file} {from:?}: {v}");
        owner_path(&fake, &v, file);
        assert_eq!(sign(&fake, &k, "session_o").0, 1, "{file} {from:?}");
    }
}

#[test]
fn a_halted_bosun_counts_no_signature() {
    let k = key();
    let fake = quorum(&k);
    fake.variable("BOSUN_HALT", "1");
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    assert!(
        description(&fake).starts_with(HALTED),
        "{}",
        description(&fake)
    );
    assert!(!description(&fake).starts_with(WAITING));
    assert_eq!(sign(&fake, &k, "session_o").0, 4);
}

#[test]
fn an_unreadable_halt_state_counts_no_signature() {
    let k = key();
    let fake = quorum(&k);
    fake.fail_path("/issues", 500);
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    assert!(
        description(&fake).starts_with(UNREADABLE),
        "{}",
        description(&fake)
    );
    assert!(!description(&fake).starts_with(WAITING));
}

#[test]
fn the_policy_is_read_from_the_base_never_the_head() {
    let k = key();
    // The head sets quorum, the base says owner: nothing counts.
    let fake = pr(&k, GATE, "owner");
    fake.content(".bosun/bosun.toml", HEAD, &config("quorum"));
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(WAITING));
    assert!(v["data"].get("policy").is_none());
    // And a PR that sets it in `.bosun/bosun.toml` is the trust root anyway.
    let fake = pr(&k, ".bosun/bosun.toml", "quorum");
    fake.content(".bosun/bosun.toml", HEAD, &config("quorum"));
    let (code, _) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5);
}

#[test]
fn in_bosun_the_judging_code_is_owner_only_and_elsewhere_it_is_not() {
    for file in [
        "src/core/risk.rs",
        "src/core/approve.rs",
        "src/cli/risk_check.rs",
    ] {
        let k = key();
        let fake = pr(&k, file, "quorum");
        fake.content("src/cli/risk_check.rs", BASE, "// marker");
        let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
        assert_eq!(code, 5, "{file}: {v}");
        owner_path(&fake, &v, file);
        assert_eq!(sign(&fake, &k, "session_o").0, 1, "{file}");
        // Without the marker at base it is some other repo's file.
        let fake = pr(&k, file, "quorum");
        let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
        assert_eq!(code, 0, "{file}: {v}");
    }
}

/// An R-secrets content hit (a fixture line, not a real secret: the reviewers judge that, D-022)
/// counts with the quorum.
#[test]
fn a_secrets_match_counts_with_the_quorum() {
    let k = key();
    let fake = quorum(&k);
    fake.pull_file(
        1,
        "src/a.env",
        "modified",
        1,
        0,
        Some("@@ -0,0 +1,1 @@\n+API_KEY = abc"),
    );
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn an_unlinked_pr_or_an_unleased_task_gets_no_quorum() {
    let k = key();
    let fake = quorum(&k);
    fake.pull_set(1, "body", json!(""));
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(WAITING));
}

/// The quorum is never implied by a mode (bosun#706): each repo sets `policy = "quorum"` in one
/// owner-approved PR, so `autonomous` and `booster` both preset the D-017 policy.
#[test]
fn no_mode_preset_is_a_quorum() {
    for mode in ["autonomous", "booster"] {
        let k = key();
        let fake = pr(&k, GATE, "owner");
        let c = format!("project = \"widget\"\n[mode]\nname = \"{mode}\"\n");
        fake.content(".bosun/bosun.toml", BASE, &c);
        let (_, v) = with(&fake, &k, &["session_r", "session_s"]);
        assert_eq!(v["data"]["policy"], "autonomous", "{mode}: {v}");
        assert_eq!(v["data"]["policy_source"], "preset");
    }
}

/// A workflow change is trust root under quorum in every `[checks]` mode: a workflow the PR adds
/// runs as the verdict author and could post verdicts (bosun#706).
#[test]
fn a_workflow_change_is_the_owners_in_every_checks_mode() {
    let k = key();
    let fake = quorum(&k);
    fake.pull_file(1, ".github/workflows/x.yml", "added", 1, 0, Some(PLAIN));
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    owner_path(&fake, &v, "workflow, statuses mode");
    let fake = quorum(&k);
    fake.content(".bosun/bosun.toml", BASE, &config_checks("quorum"));
    fake.pull_file(1, ".github/workflows/x.yml", "added", 1, 0, Some(PLAIN));
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    owner_path(&fake, &v, "workflow, checks mode");
}

/// A strong secret (a PEM private key) in the added lines is the owner's under quorum: two
/// verdicts never cover a real credential.
#[test]
fn a_strong_secret_is_the_owners_under_quorum() {
    let k = key();
    let fake = quorum(&k);
    let pem = "@@ -0,0 +1,1 @@\n+-----BEGIN PRIVATE KEY-----";
    fake.pull_file(1, "docs/notes.txt", "added", 1, 0, Some(pem));
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    owner_path(&fake, &v, "strong secret");
}

/// A trust-root change is high risk and the owner's whatever the base rules say: a base whose
/// rules match nothing still waits for the owner (bosun#706 finding 1).
#[test]
fn a_trust_root_change_is_the_owners_even_with_empty_rules() {
    for f in [
        ".bosun/risk.toml",
        ".bosun/approvers.toml",
        "src/core/risk.rs",
    ] {
        let k = key();
        let fake = pr(&k, f, "quorum");
        fake.content("src/cli/risk_check.rs", BASE, "x");
        fake.content(".bosun/risk.toml", BASE, "");
        let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
        assert_eq!(code, 5, "{f}: {v}");
        owner_path(&fake, &v, f);
        assert!(description(&fake).contains("R-trust"), "{f}");
    }
}

/// One session that relays verdicts under two invented session ids is one reviewer (bosun#706).
#[test]
fn verdicts_relayed_by_one_session_count_once() {
    let k = key();
    let fake = quorum(&k);
    for s in ["session_x", "session_y"] {
        let r = json!({"bosun": "verdict", "session": s, "head_sha": HEAD, "approved": true,
            "confidence": 0.95, "major": 0, "relayed_by": "session_relay"});
        fake.comment_by(
            1,
            "github-actions[bot]",
            &format!("Review verdict: approve 0.95, 0 blocker.\n\n{r}"),
        );
    }
    post_sig(&fake, &k);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(QUORUM), "{v}");
}

fn sign(fake: &FakeGithub, k: &Key, session: &str) -> (i32, Value) {
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("key.pem");
    std::fs::write(&path, &k.1).unwrap();
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_NOW", NOW)
        .env("BOSUN_SESSION", session)
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

#[test]
fn approve_signs_the_autonomous_form_after_two_clean_verdicts() {
    let k = key();
    let fake = quorum(&k);
    fake.content(".bosun/bosun.toml", BASE, &config_checks("quorum"));
    fake.label(1, "bosun:capability/weaken");
    fake.pull_file(1, "hoist.toml", "modified", 1, 0, Some(PLAIN));
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    let (code, v) = sign(&fake, &k, "session_o");
    assert_eq!(code, 0, "{v}");
    let body = v["data"]["body"].as_str().unwrap();
    assert!(
        body.contains("Signed under the quorum policy (D-022), not an owner answer"),
        "{body}"
    );
    assert!(body.starts_with(&format!("/bosun approve {HEAD} autonomous\n")));
    assert_eq!(v["data"]["policy"], "quorum");
}

#[test]
fn approve_refuses_with_fewer_than_two_counting_verdicts() {
    let k = key();
    // None, one, the same session twice, the author's, the signer's own (not counted).
    for sessions in [
        &[][..],
        &["session_r"][..],
        &["session_r", "session_r"][..],
        &["session_r", "session_w"][..],
        &["session_r", "session_o"][..],
    ] {
        let fake = quorum(&k);
        for s in sessions {
            post_verdict(&fake, s, HEAD);
        }
        let (code, v) = sign(&fake, &k, "session_o");
        assert_eq!(code, 1, "{sessions:?}: {v}");
    }
}

#[test]
fn approve_refuses_the_trust_root_and_a_halted_bosun() {
    let k = key();
    let fake = quorum(&k);
    fake.pull_file(1, ".bosun/approvers.toml", "modified", 1, 0, Some(PLAIN));
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    assert_eq!(sign(&fake, &k, "session_o").0, 1);
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    fake.variable("BOSUN_HALT", "1");
    assert_eq!(sign(&fake, &k, "session_o").0, 4);
}

#[test]
fn wave_agents_of_one_session_count_once_and_not_as_the_author() {
    let k = key();
    // S/r1 and S/r2 are one session.
    let fake = quorum(&k);
    let (code, v) = with(&fake, &k, &["session_r/r1", "session_r/r2"]);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(QUORUM));
    // Two real sessions still pass.
    let fake = quorum(&k);
    let (code, v) = with(&fake, &k, &["session_r/r1", "session_s/r1"]);
    assert_eq!(code, 0, "{v}");
    // The author (lease holder `session_w`) has wave agents too: they do not count.
    let fake = quorum(&k);
    let (code, v) = with(&fake, &k, &["session_w/x", "session_r"]);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(QUORUM));
}

#[test]
fn a_relayed_verdict_whose_relayer_holds_a_lease_does_not_count() {
    let k = key();
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    let relayed = verdict_of("session_s", HEAD, 0, true)
        .replace("\"head_sha\"", "\"relayed_by\":\"session_w\",\"head_sha\"");
    fake.comment_by(1, "github-actions[bot]", &relayed);
    post_sig(&fake, &k);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(QUORUM));
}

#[test]
fn an_edited_verdict_comment_does_not_count() {
    let k = key();
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    fake.comment_edited(
        1,
        "github-actions[bot]",
        &verdict_of("session_s", HEAD, 0, true),
    );
    post_sig(&fake, &k);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
}

#[test]
fn an_unseen_diff_gets_no_quorum_signature() {
    let k = key();
    let fake = quorum(&k);
    fake.pull_set(1, "changed_files", json!(5));
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    owner_path(&fake, &v, "unseen");
    // And `approve` refuses to sign it.
    let fake = quorum(&k);
    fake.pull_set(1, "changed_files", json!(5));
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    assert_eq!(sign(&fake, &k, "session_o").0, 1);
}

#[test]
fn trust_root_covers_the_risk_workflow_anywhere_and_in_bosun_the_judging_files() {
    for file in [".github/workflows/risk.yml", ".GitHub/Workflows/Risk.yml"] {
        let k = key();
        let fake = pr(&k, file, "quorum");
        let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
        assert_eq!(code, 5, "{file}: {v}");
        assert_eq!(sign(&fake, &k, "session_o").0, 1, "{file}");
    }
}

/// The signer-side guard (bosun#706): two wave agents of one other session are one reviewer, so
/// `bosun approve` does not sign (risk-check would not count it either).
#[test]
fn approve_counts_two_wave_agents_of_one_session_once() {
    let k = key();
    let fake = quorum(&k);
    post_verdict(&fake, "session_r/r1", HEAD);
    post_verdict(&fake, "session_r/r2", HEAD);
    let (code, v) = sign(&fake, &k, "session_o");
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("1 counted"),
        "{v}"
    );
    post_verdict(&fake, "session_s", HEAD);
    assert_eq!(sign(&fake, &k, "session_o").0, 0);
}

#[test]
fn approve_does_not_count_the_signers_wave_agents_as_reviewers() {
    let k = key();
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_o/x", HEAD);
    assert_eq!(sign(&fake, &k, "session_o").0, 1);
}

/// Quorum PR changing `.bosun/version` from `old` to `new` (`None`: deleted); in bosun's own repo
/// (the code marker at base) when `own`.
fn pin_pr(k: &Key, old: &str, new: Option<&str>, own: bool) -> FakeGithub {
    let fake = pr(k, ".bosun/version", "quorum");
    fake.content(".bosun/version", BASE, old);
    match new {
        Some(n) => fake.content(".bosun/version", HEAD, n),
        None => fake.pull_file(1, ".bosun/version", "removed", 0, 1, Some(PLAIN)),
    }
    if own {
        fake.content("src/cli/risk_check.rs", BASE, "x");
    }
    fake
}

#[test]
fn a_pin_downgrade_unknown_version_garbage_and_deletion_stay_with_the_owner() {
    for (old, new) in [
        ("0.22.0\n", Some("0.21.0\n")),
        ("0.22.0\n", Some("0.23.0\n")), // released? no tag: unknown version
        ("0.22.0\n", Some("latest\n")),
        ("0.22.0\n", Some("v0.23.0\n")),
        ("0.22.0\n", None),
    ] {
        let k = key();
        let fake = pin_pr(&k, old, new, true);
        fake.release_ref("v0.21.0"); // a downgrade to a released version is still refused
        let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
        assert_eq!(code, 5, "{new:?}: {v}");
        owner_path(&fake, &v, &format!("{new:?}"));
    }
}

#[test]
fn a_pin_upgrade_to_a_released_version_passes_quorum() {
    let k = key();
    let fake = pin_pr(&k, "0.22.0\n", Some("0.23.0\n"), true);
    fake.release_ref("v0.23.0");
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_consumer_repo_pin_bump_to_a_released_upstream_tag_passes() {
    let k = key();
    let fake = pin_pr(&k, "0.22.0\n", Some("0.23.0\n"), false);
    fake.release_ref("v0.23.0"); // on keelstackhq/keelstack, not on this repo
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_consumer_repo_pin_bump_to_a_missing_tag_stays_with_the_owner() {
    let k = key();
    let fake = pin_pr(&k, "0.22.0\n", Some("0.23.0\n"), false);
    fake.release_ref_in("o/r", "v0.23.0"); // a tag in the PR's own repo is not a release
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    owner_path(&fake, &v, "missing tag");
}

#[test]
fn a_bosun_repo_override_at_base_is_the_lookup_repo() {
    let k = key();
    let fake = pin_pr(&k, "0.22.0\n", Some("0.23.0\n"), false);
    fake.content(
        ".bosun/bosun.toml",
        BASE,
        "[approvals]\npolicy = \"quorum\"\nbosun_repo = \"acme/bosun-fork\"\n",
    );
    fake.release_ref("v0.23.0"); // the default repo is no longer asked
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    let fake = pin_pr(&k, "0.22.0\n", Some("0.23.0\n"), false);
    fake.content(
        ".bosun/bosun.toml",
        BASE,
        "[approvals]\npolicy = \"quorum\"\nbosun_repo = \"acme/bosun-fork\"\n",
    );
    fake.release_ref_in("acme/bosun-fork", "v0.23.0");
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn a_head_only_bosun_repo_override_is_ignored_and_a_bad_one_fails_closed() {
    let k = key();
    let fake = pin_pr(&k, "0.22.0\n", Some("0.23.0\n"), false);
    fake.content(
        ".bosun/bosun.toml",
        HEAD,
        "[approvals]\npolicy = \"quorum\"\nbosun_repo = \"acme/fork\"\n",
    );
    fake.release_ref_in("acme/fork", "v0.23.0");
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
    for bad in ["bosun", "a/b/c", "../x", "a/", "/b", "a b/c"] {
        let fake = pin_pr(&k, "0.22.0\n", Some("0.23.0\n"), false);
        fake.content(
            ".bosun/bosun.toml",
            BASE,
            &format!("[approvals]\npolicy = \"quorum\"\nbosun_repo = \"{bad}\"\n"),
        );
        fake.release_ref("v0.23.0");
        let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
        assert_eq!(code, 5, "{bad}: {v}");
    }
}

#[test]
fn an_api_error_looking_up_the_tag_fails_closed() {
    let k = key();
    let fake = pin_pr(&k, "0.22.0\n", Some("0.23.0\n"), false);
    fake.release_ref("v0.23.0");
    fake.fail_path("/git/ref/tags/v0.23.0", 500);
    let (code, v) = with(&fake, &k, &["session_r", "session_s"]);
    assert_eq!(code, 5, "{v}");
}

#[test]
fn approve_refuses_a_pin_downgrade_and_signs_a_released_upgrade() {
    let k = key();
    let fake = pin_pr(&k, "0.22.0\n", Some("0.21.0\n"), true);
    fake.release_ref("v0.21.0");
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    assert_eq!(sign(&fake, &k, "session_o").0, 1);
    let fake = pin_pr(&k, "0.22.0\n", Some("0.23.0\n"), true);
    fake.release_ref("v0.23.0");
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    assert_eq!(sign(&fake, &k, "session_o").0, 0);
}

/// `approve` reads the policy where risk-check does, at the PR's base (the same read as the
/// trust root): a default branch that moved to quorum after this PR's base signs nothing for a
/// PR that changes the trust root, and a base on quorum signs nothing once the default branch left
/// it (fail closed: both refs must hold quorum or autonomous).
#[test]
fn approve_reads_the_policy_at_the_base_like_the_trust_root() {
    let k = key();
    // The default branch holds quorum (as `pr` writes it) and would sign this PR; the base says
    // owner, so nothing is signed. A regression that read the default branch signs here.
    let fake = quorum(&k);
    fake.content(".bosun/bosun.toml", BASE, &config("owner"));
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    let (code, v) = sign(&fake, &k, "session_o");
    assert_eq!(code, 1, "{v}");
    assert!(v["data"]["body"].is_null(), "{v}");
    // And the trust root, with the base on quorum: only the trust-root refusal can stop it.
    let fake = quorum(&k);
    fake.pull_file(1, ".bosun/approvers.toml", "modified", 1, 0, Some(PLAIN));
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    let (code, v) = sign(&fake, &k, "session_o");
    assert_eq!(code, 1, "{v}");
    assert!(v["data"]["body"].is_null(), "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("trust root"),
        "{v}"
    );

    // The trust root with the base on owner: nothing is signed either, and the message names why.
    let fake = quorum(&k);
    fake.content(".bosun/bosun.toml", BASE, &config("owner"));
    fake.pull_file(1, ".bosun/approvers.toml", "modified", 1, 0, Some(PLAIN));
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    let (code, v) = sign(&fake, &k, "session_o");
    assert_eq!(code, 1, "{v}");
    assert!(v["data"]["body"].is_null(), "{v}");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("approval policy owner: only the owner approves"),
        "{v}"
    );

    let fake = quorum(&k);
    fake.content(".bosun/bosun.toml", MAIN, &config("owner"));
    // The fake lets a default-branch write stand for every PR base: restore this PR's base.
    fake.content(".bosun/bosun.toml", BASE, &config("quorum"));
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    let (code, v) = sign(&fake, &k, "session_o");
    assert_eq!(code, 1, "{v}");
    assert!(v["data"]["body"].is_null(), "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("looser than on the default branch"),
        "{v}"
    );
}

/// A verdict comment of `session` on HEAD with `confidence`, posted by `author`.
fn post_verdict_by(fake: &FakeGithub, author: &str, session: &str, confidence: f64) {
    let r = json!({"bosun": "verdict", "session": session, "head_sha": HEAD,
        "approved": true, "confidence": confidence, "major": 0});
    fake.comment_by(
        1,
        author,
        &format!("Review verdict: approve {confidence}, 0 blocker.\n\n{r}"),
    );
}

/// A signature of `k` (enrolled as `k1` or not) over `message`, posted as the Claude app.
fn post_sig_of(fake: &FakeGithub, k: &Key, message: &str) {
    let s = b64::encode(k.0.sign(message.as_bytes()).as_ref());
    let body = format!("/bosun approve {HEAD} autonomous\nbosun-sig ed25519 k1 {s}");
    fake.comment_by(1, "claude", &body);
    fake.last_comment(|c| c.via_app = Some("claude".into()));
}

fn autonomous_message(head: &str) -> String {
    format!("bosun-autonomous v1 acme/widget#1 {head}")
}

/// The quorum bar is 0.9 (`VERDICT_MIN_CONFIDENCE`, stricter than D-022's 0.85): 0.84 and 0.89
/// are both below it, so a verdict at either confidence is no clean verdict.
#[test]
fn a_verdict_below_the_confidence_bar_does_not_count_on_either_path() {
    for confidence in [0.84, 0.89] {
        let k = key();
        let fake = quorum(&k);
        post_verdict(&fake, "session_r", HEAD);
        post_verdict_by(&fake, "github-actions[bot]", "session_s", confidence);
        post_sig(&fake, &k);
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{confidence}: {v}");
        assert!(description(&fake).starts_with(QUORUM), "{confidence}: {v}");
        assert_eq!(v["data"]["approved_by"], Value::Null, "{confidence}: {v}");

        let fake = quorum(&k);
        post_verdict(&fake, "session_r", HEAD);
        post_verdict_by(&fake, "github-actions[bot]", "session_s", confidence);
        let (code, v) = sign(&fake, &k, "session_o");
        assert_eq!(code, 1, "{confidence}: {v}");
    }
    // The bar itself passes.
    let k = key();
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    post_verdict_by(&fake, "github-actions[bot]", "session_s", 0.9);
    assert_eq!(sign(&fake, &k, "session_o").0, 0);
}

#[test]
fn a_verdict_whose_author_is_not_the_bot_does_not_count_on_either_path() {
    for author in ["mallory", "ada", "claude"] {
        let k = key();
        let fake = quorum(&k);
        post_verdict(&fake, "session_r", HEAD);
        post_verdict_by(&fake, author, "session_s", 0.95);
        post_sig(&fake, &k);
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{author}: {v}");
        assert!(description(&fake).starts_with(QUORUM), "{author}: {v}");
        assert_eq!(v["data"]["approved_by"], Value::Null, "{author}: {v}");

        let fake = quorum(&k);
        post_verdict(&fake, "session_r", HEAD);
        post_verdict_by(&fake, author, "session_s", 0.95);
        assert_eq!(sign(&fake, &k, "session_o").0, 1, "{author}");
    }
}

#[test]
fn a_signing_key_not_in_approvers_toml_counts_and_signs_nothing() {
    let (enrolled, stranger) = (key(), key());
    let fake = quorum(&enrolled);
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    post_sig_of(&fake, &stranger, &autonomous_message(HEAD));
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert!(description(&fake).starts_with(QUORUM), "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");

    let fake = quorum(&enrolled);
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    let (code, v) = sign(&fake, &stranger, "session_o");
    assert_eq!(code, 1, "{v}");
    assert!(v["data"]["body"].is_null(), "{v}");
    let why = v["error"]["message"].as_str().unwrap();
    assert!(why.contains("not enrolled in .bosun/approvers.toml"), "{v}");
}

#[test]
fn a_bad_signature_counts_nothing() {
    let k = key();
    for message in [
        autonomous_message(OTHER),
        "bosun-autonomous v1 acme/widget#2 0123456789abcdef0123456789abcdef01234567".to_string(),
        format!("bosun-approve v1 acme/widget#1 {HEAD}"),
    ] {
        let fake = quorum(&k);
        post_verdict(&fake, "session_r", HEAD);
        post_verdict(&fake, "session_s", HEAD);
        post_sig_of(&fake, &k, &message);
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{message}: {v}");
        assert!(description(&fake).starts_with(QUORUM), "{message}: {v}");
        assert_eq!(v["data"]["approved_by"], Value::Null, "{message}: {v}");
    }
    // A signature that is not a valid ed25519 signature at all.
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    let body = format!("/bosun approve {HEAD} autonomous\nbosun-sig ed25519 k1 AAAA");
    fake.comment_by(1, "claude", &body);
    fake.last_comment(|c| c.via_app = Some("claude".into()));
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
}

/// An owner key's v1 or v2 signature stands for the owner only outside the quorum trust root
/// (spec 08): on `bosun/src` under quorum the owner needs their own review or comment.
#[test]
fn a_key_signed_owner_approval_never_counts_on_the_quorum_trust_root() {
    let v1 = format!("bosun-approve v1 acme/widget#1 {HEAD}");
    let v2 = format!("bosun-approve v2 acme/widget#1 {HEAD} after-review 7");
    for (file, message, first) in [
        ("src/feature/b.rs", &v1, format!("/bosun approve {HEAD}")),
        (
            "src/feature/b.rs",
            &v2,
            format!("/bosun approve {HEAD} after-review 7"),
        ),
    ] {
        let k = key();
        let fake = pr(&k, file, "quorum");
        fake.content("src/cli/risk_check.rs", BASE, "// marker");
        post_verdict(&fake, "session_r", HEAD);
        post_verdict(&fake, "session_s", HEAD);
        let s = b64::encode(k.0.sign(message.as_bytes()).as_ref());
        fake.comment_by(1, "claude", &format!("{first}\nbosun-sig ed25519 k1 {s}"));
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{message}: {v}");
        assert_eq!(v["data"]["approved_by"], Value::Null, "{message}: {v}");
        assert!(description(&fake).starts_with(WAITING), "{message}");
    }
    // Off the trust root under quorum a key signature (D-022) counts only together with the
    // QUORUM_VERDICTS clean verdicts: alone it is no owner approval.
    let s = |k: &Key| b64::encode(k.0.sign(v1.as_bytes()).as_ref());
    let k = key();
    let fake = quorum(&k);
    fake.comment_by(
        1,
        "claude",
        &format!("/bosun approve {HEAD}\nbosun-sig ed25519 k1 {}", s(&k)),
    );
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "a key alone: {v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
    let k = key();
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    fake.comment_by(
        1,
        "claude",
        &format!("/bosun approve {HEAD}\nbosun-sig ed25519 k1 {}", s(&k)),
    );
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "a key and one verdict: {v}");
    let k = key();
    let fake = quorum(&k);
    post_verdict(&fake, "session_r", HEAD);
    post_verdict(&fake, "session_s", HEAD);
    fake.comment_by(
        1,
        "claude",
        &format!("/bosun approve {HEAD}\nbosun-sig ed25519 k1 {}", s(&k)),
    );
    let (code, v) = check(&fake);
    assert_eq!(code, 0, "{v}");
}

/// Under quorum an owner-role key signature counts only with the quorum of clean verdicts, also
/// when no autonomous verdict can count at all: no open linked task leased by the author, or a
/// path the autonomous form never covers.
#[test]
fn a_key_signature_alone_never_counts_under_quorum_without_an_autonomous_path() {
    let v1 = format!("bosun-approve v1 acme/widget#1 {HEAD}");
    let sign = |fake: &FakeGithub, k: &Key| {
        let s = b64::encode(k.0.sign(v1.as_bytes()).as_ref());
        let body = format!("/bosun approve {HEAD}\nbosun-sig ed25519 k1 {s}");
        fake.comment_by(1, "claude", &body);
    };
    // No linked task.
    let k = key();
    let fake = quorum(&k);
    fake.pull_set(1, "body", json!(""));
    sign(&fake, &k);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "no linked task: {v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
    // A path outside what the autonomous form covers (`hoist.toml`: high risk, not in the allowed set).
    let k = key();
    let fake = pr(&k, "hoist.toml", "quorum");
    sign(&fake, &k);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "excluded path: {v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
}

/// D-022: a key never counts on the quorum trust root, whatever the file: a workflow, a
/// folder `AGENTS.md`, the Claude settings, the risk rules (and the rule files of `.bosun`).
#[test]
fn a_key_never_counts_on_the_quorum_trust_root_for_any_trust_root_file() {
    let v1 = format!("bosun-approve v1 acme/widget#1 {HEAD}");
    for file in [
        ".github/workflows/ci.yml",
        "AGENTS.md",
        ".claude/settings.json",
        ".bosun/risk.toml",
    ] {
        let k = key();
        let fake = pr(&k, file, "quorum");
        post_verdict(&fake, "session_r", HEAD);
        post_verdict(&fake, "session_s", HEAD);
        let s = b64::encode(k.0.sign(v1.as_bytes()).as_ref());
        fake.comment_by(
            1,
            "claude",
            &format!("/bosun approve {HEAD}\nbosun-sig ed25519 k1 {s}"),
        );
        post_sig(&fake, &k);
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{file}: {v}");
        assert_eq!(v["data"]["approved_by"], Value::Null, "{file}: {v}");
        assert!(description(&fake).starts_with(WAITING), "{file}");
    }
}

/// `approve --sign-key` without `--bootstrap` refuses too: it would post a v1 signature that
/// risk-check ignores on the quorum trust root.
#[test]
fn approve_refuses_any_signature_on_the_quorum_trust_root() {
    let k = key();
    let fake = pr(&k, "src/feature/b.rs", "quorum");
    fake.content("src/cli/risk_check.rs", BASE, "// marker");
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("key.pem");
    std::fs::write(&path, &k.1).unwrap();
    for extra in [&["--bootstrap"][..], &[][..]] {
        let out = bosun()
            .env("BOSUN_GITHUB_API", fake.url())
            .env("BOSUN_NOW", NOW)
            .env("BOSUN_SESSION", "session_o")
            .env("CCR_AGENT_PROXY_ENABLED", "true")
            .args(["approve", &format!("1@{HEAD}"), "--print"])
            .args(extra)
            .arg("--sign-key")
            .arg(&path)
            .output()
            .unwrap();
        let v = one_object(&out.stdout);
        assert_eq!(out.status.code(), Some(1), "{extra:?}: {v}");
        assert!(v["data"]["body"].is_null(), "{extra:?}: {v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("trust root"),
            "{extra:?}: {v}"
        );
    }
}

/// D-022, fail closed: a base `.bosun/bosun.toml` that does not parse may be a quorum config, so
/// every changed path counts as quorum trust root (owner only, no signature).
#[test]
fn a_base_config_that_does_not_parse_makes_every_path_quorum_trust_root() {
    let k = key();
    let fake = quorum(&k);
    fake.content(".bosun/bosun.toml", BASE, "[approvals\npolicy = ");
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_SESSION", "session_test")
        .env("BOSUN_NOW", NOW)
        .args(["risk-check", "1"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    let v = one_object(&out.stdout);
    assert_eq!(out.status.code(), Some(5), "{v}");
    assert_eq!(v["data"]["approved_by"], Value::Null, "{v}");
    assert!(
        err.contains("the PR changes the quorum trust root ("),
        "{err}"
    );
    assert!(err.contains("src/feature/a.rs"), "{err}");
    assert!(err.contains("base config does not parse"), "{err}");
    // The cause is not a path: the list holds paths only.
    assert!(
        err.contains("(src/feature/a.rs): signed approvals do not count"),
        "{err}"
    );
}

/// D-022, fail closed: `approve --sign-key` against a base config that does not parse signs
/// nothing, with or without `--bootstrap` (it may be a quorum config).
#[test]
fn approve_with_a_base_config_that_does_not_parse_signs_nothing() {
    let k = key();
    let dir = tempfile::tempdir().unwrap().keep();
    let path = dir.join("key.pem");
    std::fs::write(&path, &k.1).unwrap();
    for extra in [&["--bootstrap"][..], &[][..]] {
        let fake = quorum(&k);
        fake.content(".bosun/bosun.toml", BASE, "[approvals\npolicy = ");
        let before = fake.comments_of(1).len();
        let out = bosun()
            .env("BOSUN_GITHUB_API", fake.url())
            .env("BOSUN_NOW", NOW)
            .env("BOSUN_SESSION", "session_o")
            .env("CCR_AGENT_PROXY_ENABLED", "true")
            .args(["approve", &format!("1@{HEAD}")])
            .args(extra)
            .arg("--sign-key")
            .arg(&path)
            .output()
            .unwrap();
        let v = one_object(&out.stdout);
        assert_eq!(out.status.code(), Some(1), "{extra:?}: {v}");
        assert_eq!(v["ok"], false, "{extra:?}: {v}");
        assert!(v["data"]["body"].is_null(), "{extra:?}: {v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .is_some_and(|m| !m.is_empty()),
            "{extra:?}: {v}"
        );
        assert_eq!(fake.comments_of(1).len(), before, "{extra:?}: signed");
    }
}
