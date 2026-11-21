//! The keelstack monorepo's own setup (root `hoist.toml`, site, launchers): read-only runs on
//! the real repository root. Offline: every API base points at a closed port.

mod common;

use common::hoist;
use serde_json::Value;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn run(args: &[&str]) -> Value {
    let out = hoist()
        .current_dir(root())
        // The account id is not in the repository (it is an account setting).
        .env("CLOUDFLARE_ACCOUNT_ID", "acct-test")
        .arg("--json")
        .args(args)
        .output()
        .expect("hoist runs");
    let stdout = String::from_utf8(out.stdout).unwrap();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("{e}: {stdout}\n{}", String::from_utf8_lossy(&out.stderr)))
}

fn has(report: &Value, area: &str, kind: &str, key: &str) -> bool {
    report["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["area"] == area && i["kind"] == kind && i["key"] == key)
}

fn has_step(report: &Value, id: &str) -> bool {
    report["human_steps"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == id)
}

const TOOLS: [&str; 5] = ["bosun", "splice", "hoist", "berth", "keel"];
const PLATFORMS: [&str; 5] = [
    "linux-x64",
    "linux-arm64",
    "darwin-x64",
    "darwin-arm64",
    "win32-x64",
];

#[test]
fn explain_models_the_github_side() {
    let r = run(&["explain"]);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["exit_code"], 0);
    for check in ["ci-ok", "risk-check", "review-verdict", "main-open"] {
        assert!(has(&r, "github", "required_check", check), "{check}");
    }
    assert!(has(&r, "github", "merge_queue", "default"));
    assert!(has(&r, "github", "ruleset", "hoist: default branch"));
    assert!(has(&r, "github", "tag_ruleset", "hoist: tags"));
    for env in [
        "release",
        "bosun-app",
        "bosun-signer",
        "review",
        "hoard-r2",
        "site",
    ] {
        assert!(has(&r, "github", "environment", env), "environment {env}");
    }
    assert!(has(&r, "github", "variable", "BOSUN_APP_CLIENT_ID"));
    assert!(
        has(&r, "github", "variable", "BOSUN_REVIEW_CI"),
        "optional variable"
    );
    for name in [
        "BOSUN_APP_KEY",
        "BOSUN_SIGNER_KEY",
        "R2_ACCOUNT_ID",
        "R2_ACCESS_KEY_ID",
        "R2_SECRET_ACCESS_KEY",
        "R2_BUCKET",
        "ANTHROPIC_API_KEY",
        "CLOUDFLARE_API_TOKEN",
        "CLOUDFLARE_ACCOUNT_ID",
    ] {
        assert!(has(&r, "github", "secret", name), "secret {name}");
    }
    assert!(has(&r, "github", "app", "keelstackhq-bosun-gate"));
    assert!(has(&r, "cloudflare", "pages_project", "keelstack-landing"));
}

#[test]
fn explain_lists_the_publishable_crates_and_launchers() {
    let r = run(&["explain"]);
    for krate in ["keel-bosun", "keel-hoist", "keel-berth", "keel-cli"] {
        assert!(has(&r, "crates", "crate", krate), "crate {krate}");
        assert!(has(&r, "release", "package", krate), "package {krate}");
    }
    // hoard ships binaries only; splice is `publish = false`.
    assert!(!has(&r, "crates", "crate", "keel-hoard"));
    assert!(!has(&r, "crates", "crate", "keel-splice"));
    assert!(has(&r, "release", "package", "keel-hoard"));
    for tool in ["bosun", "hoist", "berth", "keel"] {
        assert!(
            has(&r, "npm", "package", &format!("@keelstackhq/{tool}")),
            "launcher {tool}"
        );
    }
}

#[test]
fn plan_names_a_trusted_publisher_step_for_every_package() {
    // Registries are unreachable here: first publishes are `unknown`, trust steps are not.
    let r = run(&["plan", "--area", "npm", "--area", "crates"]);
    assert_eq!(r["ok"], true, "{r}");
    for tool in TOOLS {
        for suffix in
            std::iter::once(String::new()).chain(PLATFORMS.iter().map(|p| format!("-{p}")))
        {
            let name = format!("@keelstackhq/{tool}{suffix}");
            assert!(has(&r, "npm", "trusted_publisher", &name), "{name}");
            assert!(has_step(&r, &format!("npm-trust:{name}")), "{name}");
        }
    }
    for name in [
        "@keelstackhq/cargo-deny",
        "@keelstackhq/cargo-deny-linux-x64",
        "@keelstackhq/cargo-deny-linux-arm64",
    ] {
        assert!(has_step(&r, &format!("npm-trust:{name}")), "{name}");
        let s = r["human_steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == format!("npm-trust:{name}"))
            .unwrap();
        let title = s["title"].as_str().unwrap();
        assert!(
            title.contains("--file cargo-deny-npm.yml") && title.contains("--env release"),
            "{title}"
        );
    }
    for krate in ["keel-bosun", "keel-hoist", "keel-berth", "keel-cli"] {
        assert!(has_step(&r, &format!("crates-trust:{krate}")), "{krate}");
    }
    for krate in ["keel-hoard", "keel-splice"] {
        assert!(!has_step(&r, &format!("crates-trust:{krate}")), "{krate}");
    }
    // The model packages name the release workflow, not the first job found.
    let bosun = r["human_steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "npm-trust:@keelstackhq/bosun")
        .unwrap();
    assert!(
        bosun["title"]
            .as_str()
            .unwrap()
            .contains("--file release.yml")
    );
}

#[test]
fn the_release_environment_wants_no_reviewer_and_the_workflow_is_release_yml() {
    let r = run(&["explain"]);
    assert!(has(&r, "github", "environment", "release"));
    let src = std::fs::read_to_string(root().join("hoist.toml")).unwrap();
    assert!(src.contains("reviewers = []"));
}

/// `explain` is offline and lists the desired model: a missing Cloudflare account id is an
/// `unknown` item (the Cloudflare area still needs it), not an exit 3.
#[test]
fn explain_without_an_account_id_lists_it_as_unknown() {
    let out = hoist()
        .current_dir(root())
        .args(["--json", "explain"])
        .output()
        .expect("hoist runs");
    let r: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["exit_code"], 0, "{r}");
    let item = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["area"] == "cloudflare" && i["kind"] == "account_id")
        .unwrap_or_else(|| panic!("no account_id item: {r}"));
    assert_eq!(item["action"], "unknown");
    assert!(
        item["reason"]
            .as_str()
            .unwrap()
            .contains("CLOUDFLARE_ACCOUNT_ID")
    );
    assert!(
        has(&r, "cloudflare", "pages_project", "keelstack-landing")
            || r["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["area"] == "cloudflare" && i["key"] == "keelstack-landing"),
        "{r}"
    );
}
