//! Owner team expansion in `risk-check` (spec §16, B4/T2): members of a readable team are owners
//! everywhere a login is checked; an unreadable team counts no member, says so, and a possible
//! member's request for changes withdraws (fail closed).
mod common;

use common::fake::FakeGithub;
use common::{lease_note, run};
use serde_json::{Value, json};

const NOW: &str = "2026-10-22T10:00:00Z";
const BASE: &str = "base0000";
const MAIN: &str = "refs/heads/main";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const TASK: &str = "<!-- bosun:task {} -->";
const OWNERS: &str = "[[owner]]\ngithub = \"ada\"\n[[owner]]\nteam = \"acme/core\"\n";
const TEAM_ONLY: &str = "[[owner]]\nteam = \"acme/core\"\n";
const FIX: &str = "give the bosun App Members read and pass BOSUN_APP_TOKEN, or list the owner by login in governance/owners.toml";

fn high_pr(owners: &str) -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "");
    fake.pull_as_issue(1, "change");
    fake.pull_set(1, "head", json!({"sha": HEAD, "ref": "feature"}));
    fake.content("governance/owners.toml", BASE, owners);
    let diff = Some("@@ -0,0 +1,1 @@\n+x");
    fake.pull_file(1, ".github/workflows/ci.yml", "modified", 1, 0, diff);
    fake
}

fn check(fake: &FakeGithub) -> (i32, Value) {
    run(fake, "session_test", NOW, &["risk-check", "1"])
}

#[test]
fn a_members_review_approves_and_a_non_members_does_not() {
    // Two pages of members (the fake pages by two); case does not matter.
    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["Zed", "Core-Member", "third"]);
    fake.review(1, "core-member", "APPROVED", HEAD);
    let (code, v) = check(&fake);
    assert_eq!(code, 0, "{v}");
    assert!(
        v["data"]["approved_by"]
            .as_str()
            .unwrap()
            .eq_ignore_ascii_case("core-member"),
        "{v}"
    );
    assert!(v["data"].get("owner_teams_unreadable").is_none(), "{v}");
    assert!(fake.members_calls() >= 2, "two pages read");

    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["zed", "core-member"]);
    fake.review(1, "outsider", "APPROVED", HEAD);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert!(v["data"].get("owner_teams_unreadable").is_none(), "{v}");
}

#[test]
fn a_members_approve_comment_approves_and_a_non_members_does_not() {
    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.comment_by(1, "Core-Member", &format!("/bosun approve {HEAD}"));
    let (code, v) = check(&fake);
    assert_eq!(code, 0, "{v}");

    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.comment_by(1, "outsider", &format!("/bosun approve {HEAD}"));
    assert_eq!(check(&fake).0, 5);
}

#[test]
fn a_members_request_for_changes_withdraws_their_comment() {
    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.comment_by(1, "core-member", &format!("/bosun approve {HEAD}"));
    fake.review(1, "core-member", "CHANGES_REQUESTED", HEAD);
    assert_eq!(check(&fake).0, 5);
}

#[test]
fn a_team_with_a_child_team_counts_no_member() {
    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.team_children("acme/core", &["sub"]);
    fake.review(1, "core-member", "APPROVED", HEAD);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["owner_teams_unreadable"], json!(["acme/core"]));
    assert!(v["error"]["fix"].as_str().unwrap().contains(FIX), "{v}");
    assert_eq!(fake.members_calls(), 0, "no members read past a child team");
}

#[test]
fn a_403_or_404_on_either_read_is_unreadable_and_a_listed_user_still_approves() {
    for (members, status) in [(true, 403), (true, 404), (false, 403), (false, 404)] {
        let fake = high_pr(OWNERS);
        fake.team("acme/core", &["core-member"]);
        if members {
            fake.team_members_fail("acme/core", status);
        } else {
            fake.team_children_fail("acme/core", status);
        }
        fake.review(1, "core-member", "APPROVED", HEAD);
        let (code, v) = check(&fake);
        assert_eq!(code, 5, "{members} {status}: {v}");
        assert_eq!(v["data"]["owner_teams_unreadable"], json!(["acme/core"]));
        assert!(v["error"]["fix"].as_str().unwrap().contains(FIX), "{v}");
        let status_text = fake.statuses_of(HEAD);
        let last = status_text
            .iter()
            .rev()
            .find(|s| s["context"] == "risk-check");
        let d = last.unwrap()["description"].as_str().unwrap().to_string();
        assert!(d.ends_with("(team acme/core unreadable)"), "{d}");

        // A listed user still approves; the team stays reported.
        fake.review(1, "ada", "APPROVED", HEAD);
        let (code, v) = check(&fake);
        assert_eq!(code, 0, "{v}");
        assert_eq!(v["data"]["approved_by"], "ada");
        assert_eq!(v["data"]["owner_teams_unreadable"], json!(["acme/core"]));
    }
}

#[test]
fn a_page_that_is_no_list_is_unreadable() {
    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.team_members_bad_page("acme/core");
    fake.review(1, "core-member", "APPROVED", HEAD);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["owner_teams_unreadable"], json!(["acme/core"]));
}

// RFC 8032 test 2 key (role delegate), signature over `bosun-delegate v1 acme/widget#1 <HEAD>`.
const SIG_DELEGATE: &str =
    "Yx495gcWgyARd5zAlqvj3wunBoTuAcBXFjw+nbz+G+fMohcZhSAJM/kiYFTRzdsQCBgn+uuZSUt2qkwVB5EHBg==";
const APPROVERS: &str = "[[key]]\nid = \"o1\"\ned25519 = \"11qYAYKxCrfVS/7TyWQHOg7hcvPapiMlrwIaaPcHURo=\"\nrole = \"owner\"\n\n[[key]]\nid = \"d1\"\ned25519 = \"PUAXw+hDiVqStwqnTRt+vJyYLM8uxJaMwM1V8Sr0Zgw=\"\nrole = \"delegate\"\n";
const RULES: &str = r#"
[[rule]]
id = "R-ci"
why = "ci"
paths = [".github/workflows/**", "deny.toml"]
approval = "delegate"
"#;

fn delegated(owners: &str) -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "Closes #7");
    fake.pull_as_issue(1, "change");
    fake.pull_set(1, "head", json!({"sha": HEAD, "ref": "feature"}));
    fake.issue(7, "task", &[], TASK);
    fake.comment(
        7,
        &lease_note("session_w", "2026-10-22T12:00:00Z", "src/**"),
    );
    fake.content("governance/owners.toml", BASE, owners);
    fake.content(".bosun/risk.toml", BASE, RULES);
    fake.content(".bosun/approvers.toml", MAIN, APPROVERS);
    fake.pull_file(
        1,
        "deny.toml",
        "modified",
        1,
        0,
        Some("@@ -0,0 +1,1 @@\n+x"),
    );
    let r = json!({"bosun": "verdict", "session": "session_r", "head_sha": HEAD,
        "approved": true, "confidence": 0.95, "major": 0});
    let v = format!("Review verdict: approve 0.95, 0 blocker.\n\n{r}");
    fake.comment_by(1, "github-actions[bot]", &v);
    let sig =
        format!("/bosun approve {HEAD}\nbosun-sig ed25519 d1 {SIG_DELEGATE}\n\n---\n_footer_");
    fake.comment_by(1, "claude", &sig);
    fake.last_comment(|c| c.via_app = Some("claude".into()));
    fake
}

#[test]
fn an_unreadable_team_and_a_possible_members_request_for_changes_withdraw_a_delegate() {
    let fake = delegated(OWNERS);
    fake.team_members_fail("acme/core", 403);
    let (code, v) = check(&fake);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["approved_by"], "delegate d1");
    assert_eq!(v["data"]["owner_teams_unreadable"], json!(["acme/core"]));

    for (assoc, blocks) in [
        ("MEMBER", true),
        ("OWNER", true),
        ("COLLABORATOR", true),
        ("CONTRIBUTOR", false),
        ("NONE", false),
    ] {
        let fake = delegated(OWNERS);
        fake.team_members_fail("acme/core", 403);
        fake.review(1, "stranger", "CHANGES_REQUESTED", HEAD);
        fake.review_association(1, assoc);
        let (code, v) = check(&fake);
        assert_eq!(code == 5, blocks, "{assoc}: {v}");
    }

    // A readable team: a non-member's request for changes is no owner's.
    let fake = delegated(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.review(1, "stranger", "CHANGES_REQUESTED", HEAD);
    fake.review_association(1, "MEMBER");
    assert_eq!(check(&fake).0, 0);
    // ... but a member's does.
    let fake = delegated(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.review(1, "core-member", "CHANGES_REQUESTED", HEAD);
    fake.review_association(1, "CONTRIBUTOR");
    assert_eq!(check(&fake).0, 5);
}

#[test]
fn a_team_alone_counts_no_signed_approval_while_unreadable() {
    let fake = delegated(TEAM_ONLY);
    fake.team_members_fail("acme/core", 404);
    let (code, v) = check(&fake);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["owner_teams_unreadable"], json!(["acme/core"]));
}

#[test]
fn members_are_read_once_per_run() {
    // The check, the delegate's halt test (a stranger's halt issue makes it read the owners again)
    // and the owner reads of the carry all ask for owners; the team is read once.
    let fake = delegated(OWNERS);
    fake.content("governance/owners.toml", MAIN, OWNERS);
    fake.team("acme/core", &["core-member", "other", "third"]);
    fake.issue(9, "halt", &["bosun:halt"], "");
    fake.issue_by(9, "stranger");
    let (code, v) = check(&fake);
    assert_eq!(code, 0, "{v}");
    assert_eq!(fake.members_calls(), 2, "one read of two pages");
}

#[test]
fn a_members_halt_issue_halts() {
    let fake = FakeGithub::start();
    for r in ["refs/heads/main", "main"] {
        fake.content("governance/owners.toml", r, OWNERS);
    }
    fake.team("acme/core", &["core-member", "other", "third"]);
    fake.issue(9, "halt", &["bosun:halt"], "");
    fake.issue_by(9, "Core-Member");
    fake.issue(4, "task", &["bosun:state/building"], "body");
    let (code, v) = run(&fake, "session_a", NOW, &["ask", "4", "Why?"]);
    assert_eq!(code, 4, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Core-Member"),
        "{v}"
    );
}

#[test]
fn a_session_without_the_app_token_reads_with_its_own_token() {
    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.review(1, "core-member", "APPROVED", HEAD);
    let (code, v) = check(&fake);
    assert_eq!(code, 0, "{v}");
    assert!(!fake.team_read_auth().is_empty());
}

const APP_TOKEN: &str = "app-token-canary";

/// `risk-check 1` with `BOSUN_APP_TOKEN` set: the exit code, the JSON and stdout + stderr.
fn check_with_app_token(fake: &FakeGithub) -> (i32, Value, String) {
    let out = common::bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_SESSION", "session_test")
        .env("BOSUN_NOW", NOW)
        .env("BOSUN_APP_TOKEN", APP_TOKEN)
        .args(["risk-check", "1"])
        .output()
        .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (
        out.status.code().unwrap(),
        common::one_object(&out.stdout),
        all,
    )
}

#[test]
fn team_reads_use_the_app_token_when_it_is_set() {
    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.review(1, "core-member", "APPROVED", HEAD);
    let (code, v, all) = check_with_app_token(&fake);
    assert_eq!(code, 0, "{v}");
    let auth = fake.team_read_auth();
    assert!(!auth.is_empty());
    assert!(
        auth.iter().all(|a| a.contains(APP_TOKEN)),
        "every team read carries the App token: {auth:?}"
    );
    assert!(!all.contains(APP_TOKEN), "the App token leaked: {all}");
}

#[test]
fn the_app_token_never_reaches_the_output_when_a_team_read_fails() {
    for status in [403, 404, 500] {
        let fake = high_pr(OWNERS);
        fake.team("acme/core", &["core-member"]);
        fake.team_members_fail("acme/core", status);
        fake.review(1, "core-member", "APPROVED", HEAD);
        let (code, v, all) = check_with_app_token(&fake);
        assert_eq!(code, 5, "{status}: {v}");
        assert_eq!(v["data"]["owner_teams_unreadable"], json!(["acme/core"]));
        assert!(
            !all.contains(APP_TOKEN),
            "{status}: the App token leaked: {all}"
        );
    }
    // A failing read that echoes the Authorization header as its message must not leak either.
    let fake = high_pr(OWNERS);
    fake.team("acme/core", &["core-member"]);
    fake.echo_auth_on_fail();
    fake.fail_path("/teams/core/members", 403);
    let (code, v, all) = check_with_app_token(&fake);
    assert_eq!(code, 5, "{v}");
    assert!(!all.contains(APP_TOKEN), "the App token leaked: {all}");
}
