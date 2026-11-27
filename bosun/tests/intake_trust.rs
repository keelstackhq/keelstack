//! `[intake] trusted_authors` (spec §3, BA): claim, wave plan, spawn pick-up and comment records.

mod common;

use common::fake::FakeGithub;
use common::{lease_note, run};
use serde_json::Value;

const NOW: &str = "2026-10-22T10:00:00Z";
const LATER: &str = "2026-10-22T10:20:00Z";

fn start() -> FakeGithub {
    let fake = FakeGithub::start();
    fake.issue(10, "Epic", &["bosun:state/claimed"], "");
    fake
}

fn config(fake: &FakeGithub, intake_table: &str) {
    fake.content(
        ".bosun/bosun.toml",
        "main",
        &format!("project = \"widget\"\n{intake_table}"),
    );
}

fn task(fake: &FakeGithub, n: u64, touch: &str, author: &str, assoc: Option<&str>) {
    let body = format!(
        "Do it.\n\n<!-- bosun:task {{\"item\":\"M1\",\"task\":\"T{n}\",\"epic\":10,\"touch\":[\"{touch}\"],\"kind\":\"feature\"}} -->"
    );
    fake.issue(n, &format!("task {n}"), &["bosun:state/ready"], &body);
    fake.issue_by(n, author);
    if let Some(a) = assoc {
        fake.issue_association(n, a);
    }
}

fn claim(fake: &FakeGithub, n: u64) -> (i32, Value) {
    run(fake, "session_a", NOW, &["claim", &n.to_string()])
}

fn state_of(fake: &FakeGithub, n: u64) -> Vec<String> {
    fake.issue_state(n).labels
}

#[test]
fn members_refuses_to_claim_an_outsiders_issue() {
    let fake = start();
    config(&fake, "[intake]\ntrusted_authors = \"members\"\n");
    task(&fake, 11, "src/a/**", "mallory", Some("NONE"));
    let (code, v) = claim(&fake, 11);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["reason"], "untrusted_author", "{v}");
    assert_eq!(state_of(&fake, 11), ["bosun:state/ready"]);
    assert!(fake.comments_of(11).is_empty());
}

#[test]
fn members_fails_closed_without_an_association() {
    let fake = start();
    config(&fake, "[intake]\ntrusted_authors = \"members\"\n");
    task(&fake, 11, "src/a/**", "mallory", None);
    let (code, v) = claim(&fake, 11);
    assert_eq!(code, 5, "{v}");
}

#[test]
fn members_trusts_associations_the_owner_and_owners_toml() {
    for (author, assoc) in [
        ("zed", Some("OWNER")),
        ("zed", Some("MEMBER")),
        ("zed", Some("COLLABORATOR")),
        ("acme", None),
        ("ada", None),
    ] {
        let fake = start();
        config(&fake, "[intake]\ntrusted_authors = \"members\"\n");
        fake.content(
            "governance/owners.toml",
            "main",
            "[[owner]]\ngithub = \"ada\"\n",
        );
        task(&fake, 11, "src/a/**", author, assoc);
        let (code, v) = claim(&fake, 11);
        assert_eq!(code, 0, "{author} {assoc:?}: {v}");
    }
}

#[test]
fn a_login_list_trusts_only_those_logins_and_the_owners() {
    let list = "[intake]\ntrusted_authors = [\"bo\"]\n";
    for (author, want) in [("bo", 0), ("zed", 5), ("acme", 0)] {
        let fake = start();
        config(&fake, list);
        task(&fake, 11, "src/a/**", author, Some("OWNER"));
        let (code, v) = claim(&fake, 11);
        assert_eq!(code, want, "{author}: {v}");
    }
}

#[test]
fn any_is_the_default_and_changes_nothing() {
    for cfg in [None, Some("[intake]\ntrusted_authors = \"any\"\n")] {
        let fake = start();
        if let Some(c) = cfg {
            config(&fake, c);
        }
        task(&fake, 11, "src/a/**", "mallory", Some("NONE"));
        let (code, v) = claim(&fake, 11);
        assert_eq!(code, 0, "{v}");
    }
}

#[test]
fn wave_plan_lists_the_skipped_issue_with_its_reason() {
    let fake = start();
    config(&fake, "[intake]\ntrusted_authors = \"members\"\n");
    task(&fake, 11, "src/a/**", "mallory", Some("NONE"));
    task(&fake, 12, "src/b/**", "zed", Some("MEMBER"));
    let (code, v) = run(&fake, "session_test", NOW, &["wave", "plan", "10"]);
    assert_eq!(code, 0, "{v}");
    let run_list: Vec<u64> = v["data"]["run"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["issue"].as_u64().unwrap())
        .collect();
    assert_eq!(run_list, [12], "{v}");
    let skipped = v["data"]["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1, "{v}");
    assert_eq!(skipped[0]["issue"], 11);
    assert!(
        skipped[0]["reason"].as_str().unwrap().contains("mallory"),
        "{v}"
    );
}

#[test]
fn wave_plan_skips_nothing_by_default() {
    let fake = start();
    task(&fake, 11, "src/a/**", "mallory", Some("NONE"));
    let (code, v) = run(&fake, "session_test", NOW, &["wave", "plan", "10"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["skipped"].as_array().unwrap().len(), 0, "{v}");
    assert_eq!(v["data"]["run"].as_array().unwrap().len(), 1, "{v}");
}

#[test]
fn spawn_refuses_an_outsiders_epic() {
    let fake = start();
    config(&fake, "[intake]\ntrusted_authors = \"members\"\n");
    fake.issue(
        10,
        "Epic",
        &["bosun:state/ready"],
        "<!-- bosun:epic {\"item\":\"M1\"} -->\n",
    );
    fake.issue_by(10, "mallory");
    fake.issue_association(10, "NONE");
    let (code, v) = run(&fake, "root_a", NOW, &["spawn", "10"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["reason"], "untrusted_author", "{v}");
    assert!(v["next"].as_array().is_none_or(Vec::is_empty), "{v}");
}

#[test]
fn comments_from_untrusted_authors_are_ignored() {
    // An outsider's lease on src/a/** blocks another claim today; under members it is ignored.
    for (cfg, want) in [
        (None, 3),
        (Some("[intake]\ntrusted_authors = \"members\"\n"), 0),
    ] {
        let fake = start();
        if let Some(c) = cfg {
            config(&fake, c);
        }
        fake.issue(11, "held", &["bosun:state/claimed"], "");
        task(&fake, 12, "src/a/x.rs", "octocat", Some("OWNER"));
        fake.comment_by(11, "mallory", &lease_note("session_w", LATER, "src/a/**"));
        fake.last_comment(|c| c.association = "NONE".into());
        let (code, v) = claim(&fake, 12);
        assert_eq!(code, want, "{cfg:?}: {v}");
    }
}

#[test]
fn the_config_reads_the_table_and_keeps_the_flag() {
    use bosun::config::Config;
    use bosun::core::intake_trust::TrustedAuthors as T;
    let p = |t: &str| Config::parse(t, "bosun.toml");
    assert_eq!(p("").unwrap().trusted_authors, T::Any);
    assert_eq!(p("intake = true\n").unwrap().trusted_authors, T::Any);
    let c = p("[intake]\ntrusted_authors = \"members\"\n").unwrap();
    assert_eq!((c.trusted_authors, c.intake), (T::Members, false));
    let c = p("[intake]\ntrusted_authors = [\"a\", \"b\"]\n").unwrap();
    assert_eq!(c.trusted_authors, T::Logins(vec!["a".into(), "b".into()]));
    for bad in [
        "[intake]\ntrusted_authors = \"everyone\"\n",
        "[intake]\ntrusted_authors = 3\n",
        "[intake]\nother = 1\n",
    ] {
        let e = p(bad).unwrap_err();
        assert!(!e.fix.is_empty(), "{bad}");
    }
}
