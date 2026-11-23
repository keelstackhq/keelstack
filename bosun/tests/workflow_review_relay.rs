//! Guard for the review relay (B2W/T5): `review-relay.yml` holds nothing worth stealing, and
//! risk.yml re-judges on its completion without trusting the payload beyond the head sha.

use std::fs;

fn read(name: &str) -> String {
    let path = format!("{}/../.github/workflows/{name}", env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(path).unwrap()
}

/// The yaml without comment-only lines.
fn code(yaml: &str) -> String {
    yaml.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn relay_holds_nothing_worth_stealing() {
    let y = code(&read("review-relay.yml"));
    assert!(
        y.contains("\npermissions: {}"),
        "top-level permissions: {{}}"
    );
    assert!(y.contains("pull_request_review:"));
    assert!(y.contains("types: [submitted, dismissed]"));
    for other in [
        "pull_request:",
        "pull_request_target:",
        "issue_comment:",
        "push:",
        "workflow_run:",
        "workflow_dispatch:",
    ] {
        assert!(!y.contains(other), "relay has another trigger: {other}");
    }
    assert!(!y.contains("environment"), "relay has an environment");
    assert!(!y.contains("secrets."), "relay reads a secret");
    assert!(!y.contains("uses:"), "relay uses an action");
    assert!(!y.contains("checkout"), "relay checks out");
    assert_eq!(y.matches("run:").count(), 1, "exactly one run step");
    assert!(y.contains("name: review-relay"));
    assert_eq!(y.matches("permissions:").count(), 1, "permissions: once");
    assert!(!y.contains("${{"), "relay holds an expression");
}

#[test]
fn risk_rejudges_on_relay_completion_from_the_default_branch() {
    let y = code(&read("risk.yml"));
    assert!(y.contains("workflow_run:"));
    assert!(y.contains("workflows: [review-relay]"));
    assert!(y.contains("types: [completed]"));
    assert!(
        y.contains("github.event.workflow_run.event == 'pull_request_review'"),
        "job must filter the relay's own trigger"
    );
    // The checkout ref never names a PR head.
    let ref_lines: Vec<&str> = y
        .lines()
        .filter(|l| l.trim_start().starts_with("ref:"))
        .collect();
    assert!(!ref_lines.is_empty());
    for l in ref_lines {
        assert!(
            !l.contains("head_sha") && !l.contains("head.sha"),
            "PR head in ref: {l}"
        );
        assert!(!l.contains("workflow_run"), "payload in ref: {l}");
    }
    // The head sha reaches the script through env only.
    assert!(y.contains("HEAD_SHA: ${{ github.event.workflow_run.head_sha }}"));
    let run_start = y.rfind("        run: |").expect("run block");
    assert!(
        !y[run_start..].contains("${{"),
        "no expression inside the run script"
    );
    assert!(y.contains(r#"commits/$HEAD_SHA/pulls"#));
    assert!(y.contains(".head.sha == $sha") || y.contains(".head.sha == env.HEAD_SHA"));
    assert!(y.contains(r#".base.ref == "main""#));
    assert!(y.contains(".state == \"open\""));
}

#[test]
fn relay_runs_group_per_pr() {
    let y = code(&read("risk.yml"));
    let g = &y[y.find("concurrency:").unwrap()..y.find("permissions:").unwrap()];
    assert!(
        g.contains("workflow_run.head_sha"),
        "relay group falls back to the head sha: {g}"
    );
    assert!(
        g.contains("workflow_run.pull_requests[0].number"),
        "relay group per PR: {g}"
    );
}

#[test]
fn a_relay_never_shares_the_judge_group_of_a_new_head() {
    let y = code(&read("risk.yml"));
    let g = &y[y.find("concurrency:").unwrap()..y.find("permissions:").unwrap()];
    assert!(
        g.contains("format('-relay-{0}', github.event.workflow_run.head_sha)"),
        "relay runs get a group per head sha, apart from the judge: {g}"
    );
}
