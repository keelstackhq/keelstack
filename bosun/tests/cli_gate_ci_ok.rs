//! `bosun gate ci-ok --pr N` (spec §8, keyless fast lane): the gate App's `ci-ok` check run
//! mirrors the `ci-ok` job of the newest ci.yml run on the PR head; no status, no other file.

mod common;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::Value;

const APP_TOKEN: &str = "app-token-canary";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const CI: &str = ".github/workflows/ci.yml";

fn repo(mode: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".bosun")).unwrap();
    let text = format!(
        "[checks]\nmode = \"{mode}\"\napp_id = 4242\napp_slug = \"keelstackhq-bosun-gate\"\n"
    );
    let text = if mode == "statuses" {
        String::new()
    } else {
        text
    };
    std::fs::write(dir.path().join(".bosun/bosun.toml"), text).unwrap();
    dir
}

fn pr() -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "No link.");
    fake.pull_head_sha(1, HEAD);
    fake
}

fn mirror(fake: &FakeGithub, mode: &str, token: bool) -> (i32, Value) {
    let dir = repo(mode);
    let mut cmd = bosun();
    cmd.env("BOSUN_GITHUB_API", fake.url())
        .env_remove("BOSUN_APP_TOKEN")
        .arg("-C")
        .arg(dir.path())
        .args(["gate", "ci-ok", "--pr", "1"]);
    if token {
        cmd.env("BOSUN_APP_TOKEN", APP_TOKEN);
    }
    let out = cmd.output().unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.contains(APP_TOKEN), "the App token leaked: {all}");
    let v = one_object(&out.stdout);
    if v["ok"] == false {
        assert!(!v["error"]["fix"].as_str().unwrap().trim().is_empty());
    }
    (out.status.code().unwrap(), v)
}

fn ci_ok_runs(fake: &FakeGithub) -> Vec<Value> {
    fake.check_runs_of(HEAD)
        .into_iter()
        .filter(|r| r["name"] == "ci-ok")
        .collect()
}

#[test]
fn a_green_ci_yml_run_posts_the_apps_green_ci_ok_and_no_status() {
    let fake = pr();
    let id = fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("success"));
    let (code, v) = mirror(&fake, "checks", true);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["conclusion"], "success", "{v}");
    assert_eq!(v["data"]["run_id"], id, "{v}");
    let runs = ci_ok_runs(&fake);
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(runs[0]["conclusion"], "success");
    assert_eq!(runs[0]["app"]["id"], 4242, "{runs:?}");
    assert!(fake.statuses_of(HEAD).is_empty(), "no status");
}

#[test]
fn a_red_or_cancelled_ci_ok_job_posts_failure() {
    for c in ["failure", "cancelled", "skipped"] {
        let fake = pr();
        fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some(c));
        let (code, v) = mirror(&fake, "checks", true);
        assert_eq!(code, 0, "{c}: {v}");
        assert_eq!(ci_ok_runs(&fake)[0]["conclusion"], "failure", "{c}");
    }
    // A run without a ci-ok job: failure, never success.
    let fake = pr();
    fake.workflow_file_run(HEAD, CI, "pull_request", "completed", None);
    let (code, _) = mirror(&fake, "dual", true);
    assert_eq!(code, 0);
    assert_eq!(ci_ok_runs(&fake)[0]["conclusion"], "failure");
}

#[test]
fn the_newest_ci_yml_run_decides_and_other_files_never_count() {
    let fake = pr();
    fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("failure"));
    fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("success"));
    // A PR-added workflow with a job named ci-ok, newer still: not ci.yml.
    fake.workflow_file_run(
        HEAD,
        ".github/workflows/forge.yml",
        "pull_request",
        "completed",
        Some("success"),
    );
    let (code, v) = mirror(&fake, "checks", true);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["run_id"], 1002, "{v}");
    // Only a forged file: nothing is posted.
    let fake = pr();
    fake.workflow_file_run(
        HEAD,
        ".github/workflows/forge.yml",
        "pull_request",
        "completed",
        Some("success"),
    );
    let (code, v) = mirror(&fake, "checks", true);
    assert_eq!(code, 1, "{v}");
    assert!(ci_ok_runs(&fake).is_empty());
}

#[test]
fn a_running_ci_a_missing_token_or_statuses_mode_posts_nothing() {
    let fake = pr();
    fake.workflow_file_run(HEAD, CI, "pull_request", "in_progress", None);
    let (code, v) = mirror(&fake, "checks", true);
    assert_eq!(code, 1, "{v}");
    assert!(ci_ok_runs(&fake).is_empty());

    let fake = pr();
    fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("success"));
    let (code, v) = mirror(&fake, "checks", false);
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("BOSUN_APP_TOKEN"),
        "{v}"
    );
    let (code, v) = mirror(&fake, "statuses", true);
    assert_eq!(code, 1, "{v}");
    assert!(ci_ok_runs(&fake).is_empty());
    assert!(fake.statuses_of(HEAD).is_empty());
}
