//! `hoist check` with a required-check pin whose App slug cannot be read (private App, cloud
//! proxy refusal): the id comes from `.bosun/gate-app.toml` or `integration_id`; a refused read is
//! `unknown`, never a failure; a real mismatch still fails (spec §6.1).

mod common;

use common::{FakeApi, fixture_repo, hoist};
use serde_json::Value;

const SITE: &str = "https://github.com/acme/x.git";
const FORBIDDEN: &str = r#"{"message":"Resource not accessible by integration"}"#;

type Route = (
    &'static str,
    u16,
    &'static str,
    &'static [(&'static str, &'static str)],
);

const SECRETS_LINK: &[(&str, &str)] = &[(
    "Link",
    r#"<{base}/repos/acme/x/actions/secrets?page=2>; rel="next""#,
)];

fn fake(overrides: &[Route]) -> FakeApi {
    let mut routes: Vec<Route> = vec![
        ("GET /repos/acme/x", 200, "github/repo.json", &[]),
        (
            "GET /repos/acme/x/rulesets",
            200,
            "github/rulesets_list.json",
            &[],
        ),
        (
            "GET /repos/acme/x/rulesets/11",
            200,
            "github/ruleset_app_check.json",
            &[],
        ),
        (
            "GET /repos/acme/x/actions/permissions/workflow",
            200,
            "github/actions_permissions.json",
            &[],
        ),
        (
            "GET /repos/acme/x/environments",
            200,
            "github/environments_empty.json",
            &[],
        ),
        (
            "GET /repos/acme/x/actions/secrets",
            200,
            "github/secrets_page1.json",
            SECRETS_LINK,
        ),
        (
            "GET /repos/acme/x/actions/secrets?page=2",
            200,
            "github/secrets_page2.json",
            &[],
        ),
        (
            "GET /user/installations",
            200,
            "github/installations.json",
            &[],
        ),
        (
            "GET /user/installations/501/repositories",
            200,
            "github/installation_repos.json",
            &[],
        ),
        (
            "GET /accounts/acc/pages/projects/keelstack-landing",
            200,
            "cloudflare/pages_project.json",
            &[],
        ),
    ];
    routes.retain(|(key, ..)| !overrides.iter().any(|(k, ..)| k == key));
    routes.extend_from_slice(overrides);
    FakeApi::start(&routes)
}

fn check(dir: &std::path::Path, toml: &str, gate_app: Option<&str>, api: &FakeApi) -> (i32, Value) {
    std::fs::write(dir.join("hoist.toml"), toml).unwrap();
    if let Some(g) = gate_app {
        std::fs::create_dir_all(dir.join(".bosun")).unwrap();
        std::fs::write(dir.join(".bosun/gate-app.toml"), g).unwrap();
    }
    let pages = FakeApi::start(&[("GET /", 200, "", &[])]);
    let out = hoist()
        .env("HOIST_GITHUB_API", api.url())
        .env("HOIST_CLOUDFLARE_API", api.url())
        .env("HOIST_PAGES_BASE", pages.url())
        .env("GITHUB_TOKEN", "gh-test-token")
        .env("CLOUDFLARE_API_TOKEN", "cf-test-token")
        .env("CLOUDFLARE_ACCOUNT_ID", "acc")
        .current_dir(dir)
        .args(["check", "--json"])
        .output()
        .unwrap();
    api.assert_only_get();
    let report: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "one JSON object: {e}\n{}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (out.status.code().unwrap(), report)
}

fn item<'a>(report: &'a Value, kind: &str, key: &str) -> &'a Value {
    report["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == kind && i["key"] == key)
        .unwrap_or_else(|| panic!("no {kind} {key} in {report}"))
}

const PINNED: &str =
    "[github]\nrequired_checks = [\"ci-ok\", { context = \"acme-scan\", app = \"acme\" }]\n";
const GATE_APP: &str = "id = 987654\nslug = \"acme\"\n";

#[test]
fn an_unreadable_slug_resolves_by_the_id_in_gate_app_toml() {
    let repo = fixture_repo("site", SITE);
    let api = fake(&[("GET /apps/acme", 404, r#"{"message":"Not Found"}"#, &[])]);
    let (code, report) = check(repo.path(), PINNED, Some(GATE_APP), &api);
    assert_eq!(code, 0, "{report}");
    assert_eq!(
        item(&report, "ruleset", "hoist: default branch")["action"],
        "ok"
    );
    assert_eq!(
        item(&report, "required_check", "acme-scan")["verify"]["ok"],
        true
    );
}

#[test]
fn a_refused_slug_read_resolves_by_the_id_in_gate_app_toml() {
    let repo = fixture_repo("site", SITE);
    let api = fake(&[("GET /apps/acme", 403, FORBIDDEN, &[])]);
    let (code, report) = check(repo.path(), PINNED, Some(GATE_APP), &api);
    assert_eq!(code, 0, "{report}");
}

#[test]
fn an_integration_id_in_hoist_toml_needs_no_slug_read() {
    let repo = fixture_repo("site", SITE);
    let toml = "[github]\nrequired_checks = [\"ci-ok\", { context = \"acme-scan\", integration_id = 987654 }]\n";
    let api = fake(&[("GET /apps/acme", 403, FORBIDDEN, &[])]);
    let (code, report) = check(repo.path(), toml, None, &api);
    assert_eq!(code, 0, "{report}");
    assert!(!api.requests().iter().any(|r| r.path.starts_with("/apps/")));
}

#[test]
fn an_unreadable_slug_without_an_id_is_unknown_not_a_failure() {
    let repo = fixture_repo("site", SITE);
    let api = fake(&[("GET /apps/acme", 403, FORBIDDEN, &[])]);
    let (code, report) = check(repo.path(), PINNED, None, &api);
    assert_eq!(code, 0, "{report}");
    let it = item(&report, "ruleset", "hoist: default branch");
    assert_eq!(it["action"], "unknown", "{it}");
    assert!(it["reason"].as_str().unwrap().contains("acme"), "{it}");
}

#[test]
fn a_gate_app_for_another_slug_does_not_stand_in() {
    let repo = fixture_repo("site", SITE);
    let api = fake(&[("GET /apps/acme", 404, r#"{"message":"Not Found"}"#, &[])]);
    let other = "id = 111\nslug = \"other\"\n";
    let (code, report) = check(repo.path(), PINNED, Some(other), &api);
    assert_eq!(code, 0, "{report}");
    assert_eq!(
        item(&report, "ruleset", "hoist: default branch")["action"],
        "unknown"
    );
}

#[test]
fn a_real_id_mismatch_still_fails() {
    let repo = fixture_repo("site", SITE);
    let api = fake(&[("GET /apps/acme", 404, r#"{"message":"Not Found"}"#, &[])]);
    let wrong = "id = 111\nslug = \"acme\"\n";
    let (code, report) = check(repo.path(), PINNED, Some(wrong), &api);
    assert_eq!(code, 1, "{report}");
}

#[test]
fn a_refused_ruleset_read_is_unknown_and_exits_0() {
    let repo = fixture_repo("site", SITE);
    let api = fake(&[("GET /repos/acme/x/rulesets", 403, FORBIDDEN, &[])]);
    let (code, report) = check(repo.path(), PINNED, Some(GATE_APP), &api);
    assert_eq!(code, 0, "{report}");
    assert_eq!(
        item(&report, "ruleset", "hoist: default branch")["action"],
        "unknown"
    );
}
