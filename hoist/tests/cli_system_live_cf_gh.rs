//! `hoist system check --live`: the Cloudflare and GitHub probes against a fake API.

mod common;

use common::{FakeApi, RouteSpec, hoist};
use serde_json::Value;
use std::fs;

const NOW: &str = "2026-11-27T12:00:00Z";
const CF_TOKEN: &str = "canary-cf-token-xyz";
const GH_TOKEN: &str = "canary-gh-token-xyz";
const PAT: &str = "canary-pat-value-123";

const WORKFLOW: &str = r#"name: deploy
on: push
jobs:
  deploy:
    runs-on: ubuntu-latest
    environment: site
    steps:
      - run: echo deploy
        env:
          CLOUDFLARE_API_TOKEN: ${{ secrets.CLOUDFLARE_API_TOKEN }}
          GH_PAT: ${{ secrets.GH_PAT }}
"#;

const SYSTEM: &str = r#"version = 1

[system]
name = "acme/app"
owner = "alice"
description = "Test system"

[[provider]]
id = "cloudflare"
capability = "dns+cdn"
account = "acme"

[[provider]]
id = "github"
capability = "scm"

[[credential]]
id = "cf"
provider = "cloudflare"
kind = "api_token"
role = "deploy"
purpose = "Deploy the site"
owner = "alice"
created_at = "2026-11-01"
expires_at = "2027-01-20"
max_ttl = "90d"
scopes = ["Pages Write"]
provider_ref = "tok1"
rotation = { strategy = "auto", command = "hoist secrets rotate CLOUDFLARE_API_TOKEN" }
storage = [{ kind = "gh-env-secret", repo = "acme/app", env = "site", key = "CLOUDFLARE_API_TOKEN" }]
consumers = [{ workflow = "deploy.yml", job = "deploy", environment = "site" }]

[[credential]]
id = "pat"
provider = "github"
kind = "api_token"
role = "read"
purpose = "Read the repo"
owner = "alice"
created_at = "2026-11-01"
expires_at = "2027-02-01"
max_ttl = "120d"
rotate_every = "90d"
keyless_exempt = "no app installed"
rotation = { strategy = "manual", command = "gh auth refresh" }
storage = [{ kind = "gh-secret", repo = "acme/app", key = "GH_PAT" }]
consumers = [{ workflow = "deploy.yml", job = "deploy", environment = "site" }]
"#;

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn cf_list(status: &str, expires: &str, groups: &[&str]) -> &'static str {
    let groups: Vec<String> = groups
        .iter()
        .map(|g| format!(r#"{{"name":"{g}"}}"#))
        .collect();
    leak(format!(
        r#"{{"success":true,"result":[{{"id":"tok1","name":"hoist:acme/app:deploy","status":"{status}","issued_on":"2026-11-01T00:00:00Z","expires_on":"{expires}T00:00:00Z","last_used_on":"2026-11-20T08:00:00Z","policies":[{{"permission_groups":[{}]}}]}}],"result_info":{{"page":1,"total_pages":1}}}}"#,
        groups.join(",")
    ))
}

const CF_OK: &str = r#"{"success":true,"result":[{"id":"tok1","name":"hoist:acme/app:deploy","status":"active","issued_on":"2026-11-01T00:00:00Z","expires_on":"2027-01-20T00:00:00Z","last_used_on":"2026-11-20T08:00:00Z","policies":[{"permission_groups":[{"name":"Pages Write"},{"name":"Zone Read"}]}]}],"result_info":{"page":1,"total_pages":1}}"#;
const CF_EMPTY: &str = r#"{"success":true,"result":[],"result_info":{"page":1,"total_pages":1}}"#;
const CF_VERIFY: &str = r#"{"success":true,"result":{"id":"tok1","status":"active"}}"#;
const ENV_SECRETS: &str = r#"{"total_count":1,"secrets":[{"name":"CLOUDFLARE_API_TOKEN","created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-02T00:00:00Z"}]}"#;
const REPO_SECRETS: &str = r#"{"total_count":1,"secrets":[{"name":"GH_PAT","created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-02T00:00:00Z"}]}"#;
const USER: &str = r#"{"login":"alice"}"#;
const FORBIDDEN: &str = r#"{"message":"Resource not accessible by personal access token"}"#;
const PAT_EXPIRY: (&str, &str) = (
    "github-authentication-token-expiration",
    "2027-02-01 00:00:00 UTC",
);

const CF_LIST: &str = "GET /user/tokens";
const CF_VERIFY_KEY: &str = "GET /user/tokens/verify";
const ENV_KEY: &str = "GET /repos/acme/app/environments/site/secrets";
const REPO_KEY: &str = "GET /repos/acme/app/actions/secrets";

struct Env {
    cf: bool,
    gh: bool,
    pat: bool,
}

const ALL: Env = Env {
    cf: true,
    gh: true,
    pat: true,
};

/// Run `system check --live -v` against `routes`; returns (exit code, report, all output).
fn check(routes: &[RouteSpec], env: Env) -> (i32, Value, FakeApi) {
    let api = FakeApi::start(routes);
    let dir = tempfile::tempdir_in(common::tmpdir()).unwrap();
    let wf = dir.path().join(".github/workflows");
    fs::create_dir_all(&wf).unwrap();
    fs::write(wf.join("deploy.yml"), WORKFLOW).unwrap();
    fs::write(dir.path().join("system.toml"), SYSTEM).unwrap();
    let mut cmd = hoist();
    cmd.env("HOIST_NOW", NOW)
        .env("HOIST_MAX_SLEEP_MS", "0")
        .env("HOIST_GITHUB_API", api.url())
        .env("HOIST_CLOUDFLARE_API", api.url());
    if env.cf {
        cmd.env("CLOUDFLARE_API_TOKEN", CF_TOKEN);
    }
    if env.gh {
        cmd.env("GITHUB_TOKEN", GH_TOKEN);
    }
    if env.pat {
        cmd.env("GH_PAT", PAT);
    }
    let out = cmd
        .arg("-C")
        .arg(dir.path())
        .args(["system", "check", "--live", "-v", "--json"])
        .output()
        .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for canary in [CF_TOKEN, GH_TOKEN, PAT] {
        assert!(!all.contains(canary), "{canary} leaked:\n{all}");
    }
    api.assert_only_get();
    let report: Value = serde_json::from_slice(&out.stdout).expect("one JSON object");
    (out.status.code().unwrap(), report, api)
}

fn find<'a>(r: &'a Value, kind: &str, key: &str) -> &'a Value {
    r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == kind && i["key"] == key)
        .unwrap_or_else(|| panic!("no {kind} {key} in {r}"))
}

fn reason(r: &Value, kind: &str, key: &str) -> String {
    let i = find(r, kind, key);
    format!("{} {}", i["action"], i["reason"])
}

fn good<'a>() -> Vec<RouteSpec<'a>> {
    vec![
        (CF_LIST, 200, CF_OK, &[]),
        (CF_VERIFY_KEY, 200, CF_VERIFY, &[]),
        (ENV_KEY, 200, ENV_SECRETS, &[]),
        (REPO_KEY, 200, REPO_SECRETS, &[]),
        ("GET /user", 200, USER, &[PAT_EXPIRY]),
    ]
}

fn with<'a>(key: &str, status: u16, body: &'a str) -> Vec<RouteSpec<'a>> {
    let mut v: Vec<RouteSpec<'a>> = good().into_iter().filter(|r| r.0 != key).collect();
    v.push((leak(key.to_string()), status, body, &[]));
    v
}

#[test]
fn everything_in_sync_exits_0() {
    let (code, r, _api) = check(&good(), ALL);
    assert_eq!(code, 0, "{r}");
    let cf = reason(&r, "live", "cf");
    assert!(cf.contains("ok"), "{cf}");
    assert!(
        cf.contains("active, expires 2027-01-20, issued 2026-11-01"),
        "{cf}"
    );
    assert!(cf.contains("last used 2026-11-20"), "{cf}");
    assert!(cf.contains("verified in use"), "{cf}");
    let pat = reason(&r, "live", "pat");
    assert!(pat.contains("PAT expires 2027-02-01"), "{pat}");
    assert!(reason(&r, "live_storage", "cf").contains("ok"));
    assert!(reason(&r, "live_storage", "pat").contains("ok"));
}

#[test]
fn verify_mismatch_only_omits_the_suffix() {
    let body = r#"{"success":true,"result":{"id":"other","status":"active"}}"#;
    let (code, r, _api) = check(&with(CF_VERIFY_KEY, 200, body), ALL);
    assert_eq!(code, 0, "{r}");
    let cf = reason(&r, "live", "cf");
    assert!(cf.contains("ok") && !cf.contains("verified"), "{cf}");
}

#[test]
fn revoked_token_is_drift() {
    let body = cf_list("disabled", "2027-01-20", &["Pages Write"]);
    let (code, r, _api) = check(&with(CF_LIST, 200, body), ALL);
    assert_eq!(code, 1);
    let s = reason(&r, "live", "cf");
    assert!(s.contains("drift") && s.contains("disabled"), "{s}");
}

#[test]
fn expired_token_is_drift() {
    let body = cf_list("active", "2026-11-01", &["Pages Write"]);
    let (code, r, _api) = check(&with(CF_LIST, 200, body), ALL);
    assert_eq!(code, 1);
    assert!(reason(&r, "live", "cf").contains("expired"), "{r}");
}

#[test]
fn scope_mismatch_is_drift_naming_the_missing_scope() {
    let body = cf_list("active", "2027-01-20", &["Zone Read"]);
    let (code, r, _api) = check(&with(CF_LIST, 200, body), ALL);
    assert_eq!(code, 1);
    let s = reason(&r, "live", "cf");
    assert!(s.contains("drift") && s.contains("Pages Write"), "{s}");
}

#[test]
fn scopes_match_case_insensitively() {
    let body = cf_list("active", "2027-01-20", &["pages write"]);
    let (code, r, _api) = check(&with(CF_LIST, 200, body), ALL);
    assert_eq!(code, 0, "{r}");
}

#[test]
fn token_not_listed_is_drift() {
    let (code, r, _api) = check(&with(CF_LIST, 200, CF_EMPTY), ALL);
    assert_eq!(code, 1);
    assert!(reason(&r, "live", "cf").contains("tok1 not found"), "{r}");
}

#[test]
fn no_cloudflare_token_is_unknown_with_the_fix() {
    let (code, r, api) = check(&good(), Env { cf: false, ..ALL });
    assert_eq!(code, 0, "{r}");
    let s = reason(&r, "live", "cf");
    assert!(
        s.contains("unknown") && s.contains("CLOUDFLARE_API_TOKEN"),
        "{s}"
    );
    assert!(
        api.requests()
            .iter()
            .all(|q| !q.path.starts_with("/user/tokens"))
    );
}

#[test]
fn cloudflare_401_and_403_are_unknown() {
    for status in [401, 403] {
        let (code, r, _api) = check(&with(CF_LIST, status, r#"{"success":false}"#), ALL);
        assert_eq!(code, 0, "{status}: {r}");
        let s = reason(&r, "live", "cf");
        assert!(s.contains("unknown"), "{status}: {s}");
        assert!(s.contains("CLOUDFLARE_API_TOKEN"), "{status}: {s}");
    }
}

#[test]
fn missing_storage_and_undeclared_remote_secret_are_drift() {
    let body = r#"{"total_count":1,"secrets":[{"name":"STRAY","created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-02T00:00:00Z"}]}"#;
    let (code, r, _api) = check(&with(ENV_KEY, 200, body), ALL);
    assert_eq!(code, 1);
    let s = reason(&r, "live_storage", "cf");
    assert!(
        s.contains("drift") && s.contains("missing") && s.contains("CLOUDFLARE_API_TOKEN"),
        "{s}"
    );
    let u = find(&r, "undeclared", "STRAY");
    assert_eq!(u["action"], "drift");
    assert_eq!(u["source"], "gh-env:site");
    assert!(
        u["reason"]
            .as_str()
            .unwrap()
            .contains("no [[credential]] storage names it")
    );
}

#[test]
fn secret_older_than_rotate_every_is_drift() {
    let body = r#"{"total_count":1,"secrets":[{"name":"GH_PAT","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}]}"#;
    let (code, r, _api) = check(&with(REPO_KEY, 200, body), ALL);
    assert_eq!(code, 1);
    let s = reason(&r, "live_storage", "pat");
    assert!(
        s.contains("older than rotate_every 90d") && s.contains("2026-01-01"),
        "{s}"
    );
}

#[test]
fn no_github_token_is_one_unknown_sweep() {
    let (code, r, api) = check(
        &good(),
        Env {
            gh: false,
            pat: false,
            ..ALL
        },
    );
    assert_eq!(code, 0, "{r}");
    let s = reason(&r, "live", "github");
    assert!(s.contains("unknown") && s.contains("GITHUB_TOKEN"), "{s}");
    assert!(api.requests().iter().all(|q| !q.path.starts_with("/repos")));
}

#[test]
fn forbidden_storage_scope_is_unknown_with_the_fix() {
    let (code, r, _api) = check(&with(ENV_KEY, 403, FORBIDDEN), ALL);
    assert_eq!(code, 0, "{r}");
    let s = reason(&r, "live_storage", "cf");
    assert!(
        s.contains("unknown") && s.contains("Secrets: read on acme/app"),
        "{s}"
    );
}

#[test]
fn pat_expiry_mismatch_is_drift() {
    let routes: Vec<RouteSpec> = good()
        .into_iter()
        .filter(|r| r.0 != "GET /user")
        .chain([(
            "GET /user",
            200,
            USER,
            &[(
                "github-authentication-token-expiration",
                "2027-03-01 00:00:00 UTC",
            )][..],
        )])
        .collect();
    let (code, r, _api) = check(&routes, ALL);
    assert_eq!(code, 1);
    assert!(reason(&r, "live", "pat").contains("drift"), "{r}");
}

#[test]
fn pat_without_expiry_header_never_expires_and_drifts_from_expires_at() {
    let (code, r, _api) = check(&with("GET /user", 200, USER), ALL);
    assert_eq!(code, 1);
    assert!(reason(&r, "live", "pat").contains("never expires"), "{r}");
}

#[test]
fn pat_401_is_unknown() {
    let (code, r, _api) = check(
        &with("GET /user", 401, r#"{"message":"Bad credentials"}"#),
        ALL,
    );
    assert_eq!(code, 0, "{r}");
    let s = reason(&r, "live", "pat");
    assert!(s.contains("unknown") && s.contains("rejected (401)"), "{s}");
}

#[test]
fn pat_value_not_in_env_is_unknown() {
    let (code, r, api) = check(&good(), Env { pat: false, ..ALL });
    assert_eq!(code, 0, "{r}");
    let s = reason(&r, "live", "pat");
    assert!(
        s.contains("unknown") && s.contains("not in env") && s.contains("GH_PAT"),
        "{s}"
    );
    assert!(api.requests().iter().all(|q| q.path != "/user"));
}
