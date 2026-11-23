//! `bosun review-ci` on a PR with several chunks: one model call per chunk, each under the cap,
//! the verdicts merged (approve only if every chunk approves, fail closed).
mod common;

use common::anthropic::FakeAnthropic;
use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::{Value, json};

const TASK_BODY: &str = "Do it.\n<!-- bosun:task {\"task\":\"T1\"} -->";

fn reply(v: &str, conf: f64) -> String {
    json!({"pr": 42, "head_sha": "sha42", "verdict": v, "confidence": conf,
        "findings": [{"severity": "minor", "file": "a.rs", "line": 3, "claim": "c", "evidence": "e"}],
        "lessons_hit": [], "escalate_risk": false})
    .to_string()
}

/// PR 42 with `files` files of a 30 KB patch each: one chunk per file.
fn setup(files: usize, config: &str, text: &str) -> (FakeGithub, FakeAnthropic, tempfile::TempDir) {
    let gh = FakeGithub::start();
    gh.issue(7, "task", &["bosun:state/review"], TASK_BODY);
    gh.pull_as_issue(42, "the pr");
    gh.pull(42, "the pr", "claude/x", "Closes #7");
    let patch = format!("@@ -1 +1 @@\n{}", "+x".repeat(15_000));
    for i in 0..files {
        gh.pull_file(42, &format!("src/f{i}.rs"), "modified", 1, 0, Some(&patch));
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".bosun")).unwrap();
    std::fs::write(dir.path().join(".bosun/bosun.toml"), config).unwrap();
    (gh, FakeAnthropic::start(text), dir)
}

fn review_ci(gh: &FakeGithub, ai: &FakeAnthropic, dir: &tempfile::TempDir) -> (i32, Value) {
    let out = bosun()
        .current_dir(dir.path())
        .env("BOSUN_GITHUB_API", gh.url())
        .env("BOSUN_ANTHROPIC_API", ai.url())
        .env("ANTHROPIC_API_KEY", "sk-ant-test-0123456789")
        .args(["review-ci", "42"])
        .output()
        .unwrap();
    (out.status.code().unwrap(), one_object(&out.stdout))
}

const CI: &str = "[review]\nmode = \"ci\"\n";

#[test]
fn each_chunk_is_one_call_and_all_approving_chunks_approve() {
    let (gh, ai, dir) = setup(3, CI, &reply("approve", 0.9));
    let (code, v) = review_ci(&gh, &ai, &dir);
    assert_eq!(code, 0, "{v}");
    let reqs = ai.requests();
    assert_eq!(reqs.len(), 3);
    for (i, r) in reqs.iter().enumerate() {
        let user = r.body["messages"][0]["content"].as_str().unwrap();
        // Only this chunk's file is in the call.
        for j in 0..3 {
            assert_eq!(
                user.contains(&format!("src/f{j}.rs")),
                i == j,
                "call {i}: {j}"
            );
        }
    }
    let d = &v["data"];
    assert_eq!(
        (d["chunk_count"].clone(), d["chunks_reviewed"].clone()),
        (json!(3), json!(3))
    );
    assert_eq!(d["findings_count"], 3, "the union of the findings: {v}");
    assert!(
        d["findings"][1]["claim"]
            .as_str()
            .unwrap()
            .starts_with("[chunk 2] ")
    );
    assert_eq!(d["usage"]["input_tokens"], 3000);
    assert_eq!(gh.statuses_of("sha42").last().unwrap()["state"], "success");
}

#[test]
fn a_chunk_over_the_cap_is_refused_before_any_call() {
    // One file's chunk is ~31 KB with the system prompt: under 40 KB; the cap is below it.
    let (gh, ai, dir) = setup(
        3,
        "[review]\nmode = \"ci\"\nmax_diff_bytes = 20000\n",
        &reply("approve", 0.9),
    );
    let (code, v) = review_ci(&gh, &ai, &dir);
    assert_eq!(code, 1, "{v}");
    assert!(ai.requests().is_empty());
    assert_eq!(gh.statuses_of("sha42").last().unwrap()["state"], "failure");
}

#[test]
fn a_big_pr_is_reviewed_when_each_chunk_fits_but_the_sum_does_not() {
    // 3 chunks of ~31 KB: 93 KB in all, over a 60 KB cap, each chunk under it.
    let (gh, ai, dir) = setup(
        3,
        "[review]\nmode = \"ci\"\nmax_diff_bytes = 60000\n",
        &reply("approve", 0.9),
    );
    let (code, v) = review_ci(&gh, &ai, &dir);
    assert_eq!(code, 0, "{v}");
    assert_eq!(ai.requests().len(), 3);
}

#[test]
fn a_low_confidence_chunk_keeps_the_merge_from_approving() {
    // Every call gets the same answer: confidence 0.5 is the minimum, so no approval.
    let (gh, ai, dir) = setup(2, CI, &reply("approve", 0.5));
    let (code, v) = review_ci(&gh, &ai, &dir);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["approved"], false);
    assert_eq!(gh.statuses_of("sha42").last().unwrap()["state"], "failure");
}

#[test]
fn every_chunk_failing_is_an_error_with_no_status() {
    let (gh, ai, dir) = setup(2, CI, &reply("approve", 0.9));
    ai.fail(500);
    let (code, v) = review_ci(&gh, &ai, &dir);
    assert_eq!(code, 1, "{v}");
    assert!(gh.statuses_of("sha42").is_empty());
}

#[test]
fn dry_run_reports_the_chunks_reviewed() {
    let (gh, ai, dir) = setup(2, CI, &reply("approve", 0.9));
    let out = bosun()
        .current_dir(dir.path())
        .env("BOSUN_GITHUB_API", gh.url())
        .env("BOSUN_ANTHROPIC_API", ai.url())
        .env("ANTHROPIC_API_KEY", "sk-ant-test-0123456789")
        .args(["review-ci", "42", "--dry-run"])
        .output()
        .unwrap();
    let v = one_object(&out.stdout);
    assert_eq!(v["data"]["chunks_reviewed"], 2, "{v}");
    assert_eq!(v["data"]["verdict"]["chunks_reviewed"], json!([1, 2]));
    assert!(gh.statuses_of("sha42").is_empty());
}

#[test]
fn the_budget_is_checked_before_each_chunk_and_stops_the_rest() {
    // Each call costs $0.008; the daily budget is $0.01: after two calls it is spent, so the
    // third chunk is never sent (exit 6) and no status is written.
    let (gh, ai, dir) = setup(3, CI, &reply("approve", 0.9));
    gh.content(".bosun/limits.toml", "main", "[budget]\ndaily_usd = 0.01\n");
    let (code, v) = review_ci(&gh, &ai, &dir);
    assert_eq!(code, 6, "{v}");
    assert_eq!(ai.requests().len(), 2, "{v}");
    assert!(gh.statuses_of("sha42").is_empty());
}

#[test]
fn a_cost_that_cannot_be_priced_stops_the_next_chunk_with_the_limit_error() {
    // A model with no price: the cost so far is unknown, never 0, so chunk 2 is not sent.
    let (gh, ai, dir) = setup(
        3,
        "[review]\nmode = \"ci\"\nmodel = \"mystery-1\"\n",
        &reply("approve", 0.9),
    );
    let (code, v) = review_ci(&gh, &ai, &dir);
    assert_eq!(code, 6, "{v}");
    assert_eq!(ai.requests().len(), 1, "{v}");
    assert!(gh.statuses_of("sha42").is_empty());
}
