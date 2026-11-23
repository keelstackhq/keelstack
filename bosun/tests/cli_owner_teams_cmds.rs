//! Owner teams in the commands (spec §16): until expansion is built (#25) every team is
//! unreadable, so it counts no member and everything fails closed.
mod common;

use common::fake::FakeGithub;
use common::{bosun, one_object, run};
use serde_json::{Value, json};

const NOW: &str = "2026-10-22T10:00:00Z";
const TEAM_ONLY: &str = "[[owner]]\nteam = \"acme/core\"\n";
const ADA_TEAM: &str = "[[owner]]\ngithub = \"ada\"\n\n[[owner]]\nteam = \"acme/core\"\n";
const ADA: &str = "[[owner]]\ngithub = \"ada\"\n";
const FIX: &str = "give the bosun App Members read and pass BOSUN_APP_TOKEN, or list the owner by login in governance/owners.toml";
const HEAD: &str = "0000000000000000000000000000000000000001";

fn owners(fake: &FakeGithub, text: &str) {
    for r in ["refs/heads/main", "main"] {
        fake.content("governance/owners.toml", r, text);
    }
}

fn halt_by(fake: &FakeGithub, user: &str) {
    fake.issue(9, "halt", &["bosun:halt"], "");
    fake.issue_by(9, user);
}

fn halted(fake: &FakeGithub) -> (i32, Value) {
    fake.issue(4, "task", &["bosun:state/building"], "body");
    run(fake, "session_a", NOW, &["ask", "4", "Why?"])
}

#[test]
fn a_halt_by_a_non_user_halts_while_a_team_is_unreadable() {
    let fake = FakeGithub::start();
    owners(&fake, TEAM_ONLY);
    halt_by(&fake, "core-member");
    let (code, v) = halted(&fake);
    assert_eq!(code, 4, "{v}");
    let all = v.to_string();
    assert!(
        all.contains("acme/core") && all.contains("unreadable"),
        "{all}"
    );
    assert!(all.contains(FIX), "{all}");
}

#[test]
fn a_halt_by_a_listed_user_halts_and_names_them() {
    let fake = FakeGithub::start();
    owners(&fake, ADA_TEAM);
    halt_by(&fake, "ada");
    let (code, v) = halted(&fake);
    assert_eq!(code, 4, "{v}");
    assert!(
        v["error"]["message"].as_str().unwrap().contains("by ada"),
        "{v}"
    );
}

#[test]
fn a_halt_by_a_non_user_without_a_team_does_not_halt() {
    let fake = FakeGithub::start();
    owners(&fake, ADA);
    halt_by(&fake, "stranger");
    let (code, v) = halted(&fake);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn ask_mentions_the_team_and_reports_it_unreadable() {
    let fake = FakeGithub::start();
    owners(&fake, ADA_TEAM);
    let (code, v) = halted(&fake);
    assert_eq!(code, 0, "{v}");
    let body = &fake.comments_of(4)[0].body;
    assert!(body.starts_with("@ada @acme/core Why?"), "{body}");
    assert_eq!(v["data"]["owners"], json!(["ada"]));
    assert_eq!(v["data"]["owner_teams"], json!(["acme/core"]));
    assert_eq!(v["data"]["owner_teams_unreadable"], json!(["acme/core"]));
}

#[test]
fn ask_with_only_a_team_still_has_someone_to_ask() {
    let fake = FakeGithub::start();
    owners(&fake, TEAM_ONLY);
    let (code, v) = halted(&fake);
    assert_eq!(code, 0, "{v}");
    assert!(fake.comments_of(4)[0].body.starts_with("@acme/core Why?"));
}

fn waiting(fake: &FakeGithub) {
    fake.pull(1, "gov", "b1", "");
    fake.pull_as_issue(1, "gov");
    fake.pull_set(
        1,
        "head",
        json!({"sha": HEAD, "ref": "b1", "label": "acme:b1"}),
    );
    fake.status(
        HEAD,
        "risk-check",
        "failure",
        "high risk, owner approval needed: R-gov",
    );
    fake.compare_file("base0000", HEAD, "governance/f.toml", "@@");
    fake.status(HEAD, "review-verdict", "success", "approved");
    fake.check_run(HEAD, "ci-ok", "completed", Some("success"));
}

fn list(fake: &FakeGithub) -> (i32, Value) {
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_OWNER_TOKEN", "canary-owner-token-7f3a")
        .env("BOSUN_HTTP_MAX_SLEEP_MS", "0")
        .args(["approve", "--list"])
        .output()
        .unwrap();
    (out.status.code().unwrap(), one_object(&out.stdout))
}

#[test]
fn approve_list_reports_the_unreadable_team() {
    let fake = FakeGithub::start();
    owners(&fake, ADA_TEAM);
    waiting(&fake);
    let (code, v) = list(&fake);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["owner_teams_unreadable"], json!(["acme/core"]));
    assert_eq!(v["data"]["owner_teams_fix"], FIX);
    assert_eq!(v["data"]["ready"][0]["pr"], 1);
    let fake = FakeGithub::start();
    owners(&fake, ADA);
    waiting(&fake);
    let (_, v) = list(&fake);
    assert!(v["data"].get("owner_teams_unreadable").is_none(), "{v}");
}

fn waiting_pr_numbers(v: &Value) -> Vec<Value> {
    v["data"]["waiting"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["pr"].clone())
        .collect()
}

#[test]
fn a_non_user_request_for_changes_blocks_while_a_team_is_unreadable() {
    let fake = FakeGithub::start();
    owners(&fake, ADA_TEAM);
    waiting(&fake);
    fake.review(1, "core-member", "CHANGES_REQUESTED", HEAD);
    let (code, v) = list(&fake);
    assert_eq!(code, 0, "{v}");
    assert_eq!(waiting_pr_numbers(&v), [json!(1)], "{v}");
    assert_eq!(v["data"]["ready"], json!([]), "{v}");
    // With no team the same review is nobody's withdrawal.
    let fake = FakeGithub::start();
    owners(&fake, ADA);
    waiting(&fake);
    fake.review(1, "core-member", "CHANGES_REQUESTED", HEAD);
    let (_, v) = list(&fake);
    assert_eq!(v["data"]["ready"][0]["pr"], 1, "{v}");
}

#[test]
fn a_non_user_approval_never_clears_or_counts() {
    let fake = FakeGithub::start();
    owners(&fake, ADA_TEAM);
    waiting(&fake);
    fake.review(1, "core-member", "CHANGES_REQUESTED", HEAD);
    fake.review(1, "core-member", "APPROVED", HEAD);
    fake.review(1, "ada", "CHANGES_REQUESTED", HEAD);
    let (_, v) = list(&fake);
    assert_eq!(waiting_pr_numbers(&v), [json!(1)], "{v}");
}

#[test]
fn notify_announces_a_halt_by_a_non_user_while_a_team_is_unreadable() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".bosun")).unwrap();
    std::fs::write(
        dir.path().join(".bosun/bosun.toml"),
        "[slack]\nchannel = \"C1\"\n",
    )
    .unwrap();
    let go = |text: &str, user: &str| {
        let fake = FakeGithub::start();
        owners(&fake, text);
        halt_by(&fake, user);
        let out = bosun()
            .current_dir(dir.path())
            .env("BOSUN_GITHUB_API", fake.url())
            .env("BOSUN_NOW", NOW)
            .args(["notify", "--channel-info", "-"])
            .write_stdin(r#"{"id":"C1","is_ext_shared":false,"is_org_shared":false,"is_shared":false,"guests":0}"#)
            .output()
            .unwrap();
        one_object(&out.stdout)
    };
    let v = go(TEAM_ONLY, "core-member");
    assert_eq!(v["data"]["pending"][0]["key"], "halt:9", "{v}");
    assert!(
        v["data"]["pending"][0]["text"]
            .as_str()
            .unwrap()
            .contains("acme/core")
    );
    let v = go(ADA_TEAM, "ada");
    assert_eq!(v["data"]["pending"][0]["key"], "halt:9", "{v}");
    let v = go(ADA, "stranger");
    assert_eq!(v["data"]["pending"], json!([]), "{v}");
}

const PUB: &str = "11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=";

/// A dismissed request for changes by a login that may be a team member (the team is unreadable)
/// is the review an owner key's v2 signature binds to: the signer reads the same list the
/// withdrawal check does.
#[test]
fn a_signature_after_a_non_users_dismissal_binds_to_that_review() {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "");
    fake.pull_as_issue(1, "change");
    fake.pull_set(1, "head", json!({"sha": HEAD, "ref": "feature"}));
    fake.content("governance/owners.toml", "base0000", ADA_TEAM);
    owners(&fake, ADA_TEAM);
    let keys = format!("[[key]]\nid = \"k1\"\ned25519 = \"{PUB}\"\n");
    fake.content(".bosun/approvers.toml", "refs/heads/main", &keys);
    fake.review(1, "core-member", "CHANGES_REQUESTED", HEAD);
    fake.review(1, "core-member", "DISMISSED", HEAD);
    fake.dismiss_event(1, 1, "2026-10-22T10:30:00Z");
    let dir = tempfile::tempdir().unwrap();
    let pem = dir.path().join("key.pem");
    std::fs::write(&pem, common::TEST_ED25519_PEM).unwrap();
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("CCR_AGENT_PROXY_ENABLED", "true")
        .args(["approve", &format!("1@{HEAD}"), "--print", "--sign-key"])
        .arg(&pem)
        .output()
        .unwrap();
    let v = one_object(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{v}");
    let body = v["data"]["body"].as_str().unwrap();
    assert!(body.contains("bosun-sig ed25519 k1 "), "{v}");
}
