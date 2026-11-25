mod common;

use common::{FakeApi, RouteSpec, fixture_repo, git, hoist, set_origin_head};
use serde_json::Value;

const REMOTE: &str = "https://github.com/acme/x.git";

fn run(routes: &[RouteSpec]) -> (std::process::Output, FakeApi, Value) {
    let repo = fixture_repo("publish", REMOTE);
    let d = repo.path();
    git(d, &["checkout", "-q", "-b", "main"]);
    git(d, &["add", "-A"]);
    git(d, &["commit", "-q", "-m", "chore: init"]);
    set_origin_head(d, "main");
    let api = FakeApi::start(routes);
    let output = hoist()
        .arg("-C")
        .arg(d)
        .args(["release", "check", "--json"])
        .env("HOIST_NPM_REGISTRY", api.url())
        .env("HOIST_CRATES_API", api.url())
        .env("HOIST_GITHUB_API", api.url())
        .env("GITHUB_TOKEN", "test-token")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output, api, v)
}

fn item<'a>(v: &'a Value, kind: &str, key: &str) -> &'a Value {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == kind && i["key"] == key)
        .unwrap_or_else(|| panic!("no item {kind}/{key} in {v:#}"))
}

#[test]
fn published_at_the_workspace_version_is_ok_and_trust_is_unknown() {
    let (o, api, v) = run(&[
        (
            "GET /@acme%2Flib",
            200,
            r#"{"dist-tags":{"latest":"1.0.0"}}"#,
            &[],
        ),
        ("GET /@acme%2Flib/1.0.0", 200, "{}", &[]),
        (
            "GET /api/v1/crates/tool",
            200,
            r#"{"crate":{"max_stable_version":"0.3.0"}}"#,
            &[],
        ),
        ("GET /api/v1/crates/tool/0.3.0", 200, "{}", &[]),
    ]);
    assert_eq!(o.status.code(), Some(0), "{v:#}");
    api.assert_only_get();
    let i = item(&v, "registry", "@acme/lib@1.0.0");
    assert_eq!(i["action"], "ok");
    assert!(i["reason"].as_str().unwrap().contains("newest 1.0.0"));
    assert_eq!(item(&v, "registry", "tool@0.3.0")["action"], "ok");
    assert_eq!(
        item(&v, "trusted_publisher", "tool@0.3.0")["action"],
        "unknown"
    );
}

#[test]
fn behind_is_update_and_missing_is_pending() {
    let (o, _api, v) = run(&[
        (
            "GET /@acme%2Flib",
            200,
            r#"{"dist-tags":{"latest":"0.9.0"}}"#,
            &[],
        ),
        ("GET /@acme%2Flib/1.0.0", 404, "", &[]),
        ("GET /api/v1/crates/tool", 404, "", &[]),
        ("GET /api/v1/crates/tool/0.3.0", 404, "", &[]),
    ]);
    assert_eq!(o.status.code(), Some(1), "{v:#}");
    let behind = item(&v, "registry", "@acme/lib@1.0.0");
    assert_eq!(behind["action"], "update");
    assert!(
        behind["reason"]
            .as_str()
            .unwrap()
            .contains("newest on npm is 0.9.0")
    );
    let missing = item(&v, "registry", "tool@0.3.0");
    assert_eq!(missing["action"], "pending");
    assert!(
        missing["reason"]
            .as_str()
            .unwrap()
            .contains("hoist release bootstrap")
    );
}

#[test]
fn an_unreadable_registry_is_unknown_and_reads_send_no_authorization() {
    let (o, api, v) = run(&[
        ("GET /@acme%2Flib", 503, r#"{"message":"down"}"#, &[]),
        ("GET /@acme%2Flib/1.0.0", 503, r#"{"message":"down"}"#, &[]),
        // npm answered without a `latest` dist-tag: unknown, never an empty version.
        ("GET /api/v1/crates/tool", 200, r#"{"crate":{}}"#, &[]),
        ("GET /api/v1/crates/tool/0.3.0", 200, "{}", &[]),
    ]);
    assert_eq!(o.status.code(), Some(0), "{v:#}");
    assert_eq!(item(&v, "registry", "@acme/lib@1.0.0")["action"], "unknown");
    assert_eq!(item(&v, "registry", "tool@0.3.0")["action"], "unknown");
    assert!(
        !v["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "trusted_publisher"),
        "no trust item for a package that could not be read"
    );
    let reqs = api.requests();
    assert!(!reqs.is_empty());
    for r in reqs
        .iter()
        .filter(|r| r.path.starts_with("/@") || r.path.contains("/crates/"))
    {
        assert!(
            r.auth.as_deref().is_none_or(|a| a.trim().is_empty()),
            "registry read with credentials: {:?}",
            r.auth
        );
    }
}
