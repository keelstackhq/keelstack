//! Spec §15, Transition step 7 "never before step 6": in `checks` mode the gates fail closed
//! while the default branch's ruleset does not pin risk-check, review-verdict and main-open to
//! the gate App (`integration_id` = `[checks] app_id`). risk-check (a required check) posts a
//! failing run and exits 1, and `approve --as signer` signs nothing; the fix names
//! `hoist apply --area github`. `dual` and `statuses` never read the ruleset.

mod common;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::Value;

const APP_TOKEN: &str = "app-token-canary";
const HEAD: &str = "sha1";

fn repo(mode: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".bosun")).unwrap();
    std::fs::write(
        dir.path().join(".bosun/bosun.toml"),
        format!(
            "[checks]\nmode = \"{mode}\"\napp_id = 4242\napp_slug = \"keelstackhq-bosun-gate\"\n"
        ),
    )
    .unwrap();
    dir
}

fn pr() -> FakeGithub {
    let fake = FakeGithub::start();
    fake.pull(1, "docs", "feature", "No link.");
    fake.pull_head_sha(1, HEAD);
    fake.pull_file(
        1,
        "docs/a.md",
        "modified",
        1,
        0,
        Some("@@ -0,0 +1,1 @@\n+x"),
    );
    fake
}

fn risk_check(fake: &FakeGithub, mode: &str) -> (i32, Value) {
    let dir = repo(mode);
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_APP_TOKEN", APP_TOKEN)
        .arg("-C")
        .arg(dir.path())
        .args(["risk-check", "1"])
        .output()
        .unwrap();
    let v = one_object(&out.stdout);
    if v["ok"] == false {
        assert!(!v["error"]["fix"].as_str().unwrap().trim().is_empty());
    }
    (out.status.code().unwrap(), v)
}

fn risk_runs(fake: &FakeGithub) -> Vec<Value> {
    fake.check_runs_of(HEAD)
        .into_iter()
        .filter(|r| r["name"] == "risk-check")
        .collect()
}

#[test]
fn checks_mode_passes_once_the_gates_are_pinned_to_the_app() {
    let fake = pr();
    let (code, v) = risk_check(&fake, "checks");
    assert_eq!(code, 0, "{v}");
    assert_eq!(risk_runs(&fake).last().unwrap()["conclusion"], "success");
}

#[test]
fn checks_mode_fails_closed_while_the_ruleset_is_unpinned() {
    for (what, setup) in [
        ("no pins", None),
        (
            "one gate unpinned",
            Some(vec!["risk-check@4242", "review-verdict@4242", "main-open"]),
        ),
        (
            "pinned to another App",
            Some(vec!["risk-check@9", "review-verdict@9", "main-open@9"]),
        ),
    ] {
        let fake = pr();
        match &setup {
            None => fake.unpin_gates(),
            Some(c) => fake.enforce_checks(c),
        }
        let (code, v) = risk_check(&fake, "checks");
        assert_eq!(code, 1, "{what}: {v}");
        let msg = v["error"]["message"].as_str().unwrap();
        assert!(msg.contains("not pinned to the gate App"), "{what}: {msg}");
        assert!(
            v["error"]["fix"]
                .as_str()
                .unwrap()
                .contains("hoist apply --area github"),
            "{what}: {v}"
        );
        let runs = risk_runs(&fake);
        assert_eq!(
            runs.last().unwrap()["conclusion"],
            "failure",
            "{what}: {runs:?}"
        );
    }
}

#[test]
fn dual_mode_never_reads_the_pins() {
    let fake = pr();
    fake.unpin_gates();
    let (code, v) = risk_check(&fake, "dual");
    assert_eq!(code, 0, "{v}");
}

#[test]
fn an_unreadable_ruleset_fails_closed_in_checks_mode() {
    let fake = pr();
    fake.fail_path("/rules/branches/main", 403);
    let (code, v) = risk_check(&fake, "checks");
    assert_eq!(code, 1, "{v}");
    assert!(
        v["error"]["fix"]
            .as_str()
            .unwrap()
            .contains("hoist apply --area github"),
        "{v}"
    );
}
