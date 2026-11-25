//! `bosun gate ci-ok --pr N` (spec §8, keyless fast lane): the gate App's `bosun-ci-ok` check
//! run mirrors the `ci-ok` job of the newest ci.yml run on the PR head; no status, no other file,
//! never a run named `ci-ok` (the required check stays GitHub Actions' alone). A PR that changes
//! a workflow file the mirror trusts gets no mirror.

mod common;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::{Value, json};

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

/// ci.yml on the default branch: the mirror reads its local actions (`uses: ./...`).
const CI_YML: &str = "jobs:\n  ci-hull:\n    steps:\n      - uses: ./hoard\n        with:\n          x: 1\n  ci-ok:\n    runs-on: x\n";

fn pr() -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "No link.");
    fake.pull_head_sha(1, HEAD);
    fake.content(CI, "refs/heads/main", CI_YML);
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
        .filter(|r| r["name"] == "bosun-ci-ok")
        .collect()
}

fn named_ci_ok(fake: &FakeGithub) -> usize {
    (fake.check_runs_of(HEAD).iter())
        .filter(|r| r["name"] == "ci-ok")
        .count()
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
    assert_eq!(named_ci_ok(&fake), 0, "never a run under the required name");
}

#[test]
fn a_pr_that_changes_ci_yml_gets_no_mirror() {
    for (file, prev) in [
        (".github/workflows/ci.yml", None),
        (
            ".github/workflows/renamed.yml",
            Some(".github/workflows/ci.yml"),
        ),
    ] {
        let fake = pr();
        fake.pull_file(1, file, "modified", 1, 0, Some("@@ -0,0 +1,1 @@\n+x"));
        if let Some(p) = prev {
            fake.renamed_from(1, p);
        }
        fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("success"));
        let (code, v) = mirror(&fake, "checks", true);
        assert_eq!(code, 1, "{file}: {v}");
        assert!(
            v["error"]["message"].as_str().unwrap().contains("ci.yml"),
            "{file}: {v}"
        );
        assert!(ci_ok_runs(&fake).is_empty(), "{file}");
    }
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

#[test]
fn a_run_bound_to_another_pr_or_base_is_never_mirrored() {
    // Attack: PR 2 from the same head into a branch `evil` whose ci.yml always passes.
    for (what, prs) in [
        (
            "another PR",
            json!([{"number": 2, "base": {"ref": "main"}}]),
        ),
        (
            "another base",
            json!([{"number": 1, "base": {"ref": "evil"}}]),
        ),
        ("no PR (a fork or a push)", json!([])),
    ] {
        let fake = pr();
        fake.workflow_file_run_for(HEAD, CI, "pull_request", "completed", Some("success"), prs);
        let (code, v) = mirror(&fake, "checks", true);
        assert_eq!(code, 1, "{what}: {v}");
        assert!(ci_ok_runs(&fake).is_empty(), "{what}");
    }
    // A forged green run for another base beside a red one for this PR: the red one decides.
    let fake = pr();
    fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("failure"));
    fake.workflow_file_run_for(
        HEAD,
        CI,
        "pull_request",
        "completed",
        Some("success"),
        json!([{"number": 1, "base": {"ref": "evil"}}]),
    );
    let (code, v) = mirror(&fake, "checks", true);
    assert_eq!(code, 0, "{v}");
    assert_eq!(ci_ok_runs(&fake)[0]["conclusion"], "failure");
}

#[test]
fn a_pr_that_changes_what_ci_yml_runs_from_the_checkout_gets_no_mirror() {
    // One per class: .github/** (workflows, scripts, actions), a local `uses: ./` action ci.yml
    // runs, and every R-ci path (scripts/** at any depth, action.yml anywhere, toolchain and
    // tool configs, deny.toml, Makefile).
    for file in [
        ".github/workflows/other.yml",
        ".github/scripts/affected.sh",
        ".github/actions/x/action.yml",
        "hoard/action.yml",
        "hoard/action/dist/index.js",
        "scripts/bump-version.sh",
        "hull/scripts/render.mjs",
        "tools/x/action.yml",
        "rust-toolchain.toml",
        "deny.toml",
        ".cargo/config.toml",
        "Makefile",
    ] {
        let fake = pr();
        fake.pull_file(1, file, "modified", 1, 0, Some("@@ -0,0 +1,1 @@\n+x"));
        fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("success"));
        let (code, v) = mirror(&fake, "checks", true);
        assert_eq!(code, 1, "{file}: {v}");
        assert!(ci_ok_runs(&fake).is_empty(), "{file}");
    }
    // Other code is what ci.yml tests, not what it trusts: mirrored.
    let fake = pr();
    fake.pull_file(
        1,
        "hoist/src/a.rs",
        "modified",
        1,
        0,
        Some("@@ -0,0 +1,1 @@\n+x"),
    );
    fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("success"));
    assert_eq!(mirror(&fake, "checks", true).0, 0);
    // No ci.yml on the default branch: the local actions are unknown, so nothing is mirrored.
    let fake = FakeGithub::start();
    fake.pull(1, "change", "feature", "No link.");
    fake.pull_head_sha(1, HEAD);
    fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("success"));
    assert_eq!(mirror(&fake, "checks", true).0, 1);
}

#[test]
fn a_pr_listing_3000_files_gets_no_mirror_because_the_list_may_be_cut() {
    // GitHub's files API stops at 3000: a full list may hide a CI input, fail closed.
    for (count, want) in [(2999, 0), (3000, 1)] {
        let fake = pr();
        for i in 0..count {
            fake.pull_file(1, &format!("docs/page{i}.md"), "added", 1, 0, None);
        }
        fake.workflow_file_run(HEAD, CI, "pull_request", "completed", Some("success"));
        let (code, v) = mirror(&fake, "checks", true);
        assert_eq!(code, want, "{count} files: {v}");
        if want == 1 {
            let msg = v["error"]["message"].as_str().unwrap();
            assert!(msg.contains("3000"), "{msg}");
            assert!(ci_ok_runs(&fake).is_empty());
        }
    }
}
