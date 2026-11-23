//! `hoist org plan|check|apply` (spec §16): org.toml, GitHub, Cloudflare, npm, crates and Search Console sections.

mod common;

use common::{FakeApi, RouteSpec, hoist};
use serde_json::Value;
use tempfile::TempDir;

const GH_TOML: &str = r#"[github]
org = "acme"
apps = ["claude"]
[github.provision]
repo = "provision"
environment = "provision"
branch = "main"
secrets = ["SPLICE_BOT_TOKEN"]
[github.cloud_environment]
name = "acme"
"#;

const CF_TOML: &str = r#"[cloudflare]
account_id = "acct"
[[cloudflare.zones]]
name = "example.dev"
[[cloudflare.zones.records]]
type = "TXT"
name = "example.dev"
content = "v=spf1 -all"
[[cloudflare.pages_domains]]
project = "site"
domain = "example.dev"
"#;

const ENV: &str = "/repos/acme/provision/environments/provision";

fn dir(toml: &str) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("org.toml"), toml).unwrap();
    d
}

fn start(routes: &[(&str, u16, &str)]) -> FakeApi {
    let specs: Vec<RouteSpec> = routes
        .iter()
        .map(|(k, s, b)| (*k, *s, *b, &[][..]))
        .collect();
    FakeApi::start(&specs)
}

fn gh_routes(env_status: u16) -> Vec<(String, u16, String)> {
    let r = |k: &str, s: u16, b: &str| (k.to_string(), s, b.to_string());
    vec![
        r("GET /orgs/acme", 200, r#"{"login":"acme"}"#),
        r(
            "GET /orgs/acme/installations",
            200,
            r#"{"total_count":1,"installations":[{"app_slug":"claude","repository_selection":"all"}]}"#,
        ),
        r("GET /repos/acme/provision", 200, r#"{"name":"provision"}"#),
        r(
            &format!("GET {ENV}"),
            env_status,
            r#"{"name":"provision","protection_rules":[],"deployment_branch_policy":{"protected_branches":false,"custom_branch_policies":true}}"#,
        ),
        r(
            &format!("GET {ENV}/deployment-branch-policies"),
            200,
            r#"{"total_count":1,"branch_policies":[{"id":1,"name":"main","type":"branch"}]}"#,
        ),
        r(
            &format!("GET {ENV}/secrets"),
            200,
            r#"{"total_count":1,"secrets":[{"name":"SPLICE_BOT_TOKEN"}]}"#,
        ),
    ]
}

fn gh_api(routes: Vec<(String, u16, String)>) -> FakeApi {
    let v: Vec<(&str, u16, &str)> = routes
        .iter()
        .map(|(k, s, b)| (k.as_str(), *s, b.as_str()))
        .collect();
    start(&v)
}

fn run(args: &[&str], d: &TempDir, envs: &[(&str, String)], code: i32) -> Value {
    let mut c = hoist();
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c
        .current_dir(d.path())
        .args(["--json", "org"])
        .args(args)
        .assert()
        .code(code)
        .get_output()
        .clone();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for secret in [
        "gh-test-token",
        "ghs_x",
        "cf-test-token",
        "npm-secret-tok",
        "cio-secret-tok",
        "sc-secret-tok",
    ] {
        assert!(!all.contains(secret), "token leaked: {all}");
    }
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("one JSON object: {all}"))
}

fn gh_env(api: &FakeApi, token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("HOIST_GITHUB_API", api.url()),
        ("GITHUB_TOKEN", token.to_string()),
    ]
}

fn item<'a>(r: &'a Value, kind: &str, key: &str) -> &'a Value {
    r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == kind && i["key"] == key)
        .unwrap_or_else(|| panic!("no {kind} {key} in {r:#}"))
}

#[test]
fn missing_file_is_exit_3_naming_the_path() {
    let d = tempfile::tempdir().unwrap();
    let out = hoist()
        .current_dir(d.path())
        .args(["org", "plan"])
        .assert()
        .code(3)
        .get_output()
        .stderr
        .clone();
    assert!(String::from_utf8_lossy(&out).contains("org.toml"));
}

#[test]
fn unknown_key_is_exit_3_naming_key_and_file() {
    let d = dir("[github]\norg = \"acme\"\nbogus = 1\n");
    let out = hoist()
        .current_dir(d.path())
        .args(["org", "check"])
        .assert()
        .code(3)
        .get_output()
        .stderr
        .clone();
    let err = String::from_utf8_lossy(&out);
    assert!(err.contains("bogus") && err.contains("org.toml"), "{err}");
}

#[test]
fn validation_errors_are_exit_3() {
    for bad in [
        "[github]\norg = \"\"\n",
        "[[cloudflare.zones]]\nname = \"a.dev\"\n[[cloudflare.zones.records]]\ntype = \"MX\"\nname = \"a.dev\"\ncontent = \"x\"\n",
        "[[cloudflare.zones]]\nname = \"a.dev\"\n[[cloudflare.zones.records]]\ntype = \"A\"\nname = \"b.dev\"\ncontent = \"1.1.1.1\"\n",
        "[[cloudflare.pages_domains]]\nproject = \"s\"\ndomain = \"a.dev\"\n",
        "[npm]\nscope = \"a\"\npackages = [\"tool\"]\n",
        "[[search_console.sites]]\ndomain = \"a.dev\"\nzone = \"b.dev\"\n",
    ] {
        let d = dir(bad);
        hoist()
            .current_dir(d.path())
            .args(["org", "plan"])
            .assert()
            .code(3);
    }
}

#[test]
fn github_all_ok_is_exit_0_and_get_only() {
    let api = gh_api(gh_routes(200));
    let d = dir(GH_TOML);
    let r = run(&["check"], &d, &gh_env(&api, "gh-test-token"), 0);
    api.assert_only_get();
    assert_eq!(r["command"], "org check");
    assert_eq!(item(&r, "org", "acme")["action"], "ok");
    assert_eq!(
        item(&r, "environment", "provision/provision")["action"],
        "ok"
    );
    assert_eq!(
        item(&r, "env_secret", "provision/provision/SPLICE_BOT_TOKEN")["action"],
        "ok"
    );
    // The cloud environment is a confirm step: listed, never exit 2.
    assert_eq!(item(&r, "cloud_environment", "acme")["action"], "unknown");
    let steps = r["human_steps"].as_array().unwrap();
    assert!(
        steps
            .iter()
            .any(|s| s["id"] == "github:cloud_environment:acme" && s["status"] == "confirm")
    );
}

#[test]
fn missing_app_is_pending_with_step_and_exit_2() {
    let mut routes = gh_routes(200);
    routes[1].2 = r#"{"total_count":0,"installations":[]}"#.to_string();
    let api = gh_api(routes);
    let d = dir(GH_TOML);
    let r = run(&["check"], &d, &gh_env(&api, "gh-test-token"), 2);
    assert_eq!(item(&r, "app", "claude")["action"], "pending");
    let steps = r["human_steps"].as_array().unwrap();
    assert!(
        steps
            .iter()
            .any(|s| s["id"] == "github:app:claude" && s["status"] == "pending")
    );
    // plan exits 0 whatever it finds
    run(&["plan"], &d, &gh_env(&api, "gh-test-token"), 0);
}

#[test]
fn no_github_token_is_unknown_exit_0_and_no_request() {
    let api = gh_api(gh_routes(200));
    let d = dir(GH_TOML);
    let envs = vec![("HOIST_GITHUB_API", api.url())];
    let r = run(&["check"], &d, &envs, 0);
    assert!(api.requests().is_empty());
    assert_eq!(item(&r, "org", "acme")["action"], "unknown");
    assert!(
        item(&r, "org", "acme")["reason"]
            .as_str()
            .unwrap()
            .contains("token")
    );
}

#[test]
fn missing_environment_is_create_and_apply_outside_actions_writes_nothing() {
    let api = gh_api(gh_routes(404));
    let d = dir(GH_TOML);
    let envs = gh_env(&api, "ghs_x");
    let r = run(&["plan"], &d, &envs, 0);
    assert_eq!(
        item(&r, "environment", "provision/provision")["action"],
        "create"
    );
    // A token without GITHUB_ACTIONS=true: nothing is written.
    let r = run(&["apply", "--yes"], &d, &envs, 1);
    assert_eq!(
        item(&r, "environment", "provision/provision")["action"],
        "create"
    );
    api.assert_only_get();
}

#[test]
fn apply_in_actions_with_an_app_token_writes_the_environment() {
    let mut routes = gh_routes(404);
    routes.push((format!("PUT {ENV}"), 200, "{}".into()));
    routes.push((
        format!("POST {ENV}/deployment-branch-policies"),
        201,
        r#"{"id":2}"#.into(),
    ));
    let api = gh_api(routes);
    let d = dir(GH_TOML);
    let mut envs = gh_env(&api, "ghs_x");
    envs.push(("GITHUB_ACTIONS", "true".into()));
    // The re-read still sees the fake's 404; the write itself is what is asserted.
    run(&["apply", "--yes", "--area", "github"], &d, &envs, 1);
    let reqs = api.requests();
    let put = reqs
        .iter()
        .find(|q| q.method == "PUT" && q.path == ENV)
        .expect("PUT environment");
    let body: Value = serde_json::from_str(put.body.as_deref().unwrap()).unwrap();
    assert_eq!(
        body["deployment_branch_policy"]["custom_branch_policies"],
        true
    );
    assert_eq!(
        body["deployment_branch_policy"]["protected_branches"],
        false
    );
    assert!(body.get("reviewers").is_none());
    let post = reqs
        .iter()
        .find(|q| q.method == "POST" && q.path == format!("{ENV}/deployment-branch-policies"))
        .expect("POST policy");
    let body: Value = serde_json::from_str(post.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["name"], "main");
    assert_eq!(body["type"], "branch");
}

fn cf_routes() -> Vec<(&'static str, u16, &'static str)> {
    vec![
        (
            "GET /zones?name=example.dev",
            200,
            r#"{"success":true,"result":[{"id":"z1","name":"example.dev"}]}"#,
        ),
        (
            "GET /zones/z1/dns_records",
            200,
            r#"{"success":true,"result":[],"result_info":{"page":1,"total_pages":1}}"#,
        ),
        (
            "GET /accounts/acct/pages/projects/site/domains",
            200,
            r#"{"success":true,"result":[],"result_info":{"page":1,"total_pages":1}}"#,
        ),
        (
            "POST /zones/z1/dns_records",
            200,
            r#"{"success":true,"result":{"id":"r1"}}"#,
        ),
        (
            "POST /accounts/acct/pages/projects/site/domains",
            200,
            r#"{"success":true,"result":{"name":"example.dev"}}"#,
        ),
    ]
}

fn cf_env(api: &FakeApi) -> Vec<(&'static str, String)> {
    vec![
        ("HOIST_CLOUDFLARE_API", api.url()),
        ("CLOUDFLARE_API_TOKEN", "cf-test-token".into()),
    ]
}

#[test]
fn cloudflare_plan_shows_create_and_apply_writes_record_and_domain() {
    let api = start(&cf_routes());
    let d = dir(CF_TOML);
    let r = run(&["plan"], &d, &cf_env(&api), 0);
    api.assert_only_get();
    assert_eq!(item(&r, "zone", "example.dev")["action"], "ok");
    assert_eq!(
        item(&r, "dns_record", "TXT example.dev v=spf1 -all")["action"],
        "create"
    );
    assert_eq!(
        item(&r, "pages_domain", "site/example.dev")["action"],
        "create"
    );

    // The fake keeps answering "empty" so the re-read still shows creates: exit 1.
    run(&["apply", "--yes"], &d, &cf_env(&api), 1);
    let reqs = api.requests();
    let rec = reqs
        .iter()
        .find(|q| q.method == "POST" && q.path == "/zones/z1/dns_records")
        .expect("record POST");
    let body: Value = serde_json::from_str(rec.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["type"], "TXT");
    assert_eq!(body["content"], "v=spf1 -all");
    assert_eq!(body["ttl"], 1);
    assert!(body.get("proxied").is_none());
    let dom = reqs
        .iter()
        .find(|q| q.method == "POST" && q.path == "/accounts/acct/pages/projects/site/domains")
        .expect("domain POST");
    let body: Value = serde_json::from_str(dom.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["name"], "example.dev");
}

#[test]
fn apply_without_yes_and_without_a_terminal_is_exit_3_with_no_write() {
    let api = start(&cf_routes());
    let d = dir(CF_TOML);
    hoist()
        .current_dir(d.path())
        .envs(cf_env(&api))
        .args(["org", "apply"])
        .assert()
        .code(3);
    assert!(api.requests().iter().all(|q| q.method == "GET"));
}

#[test]
fn cloudflare_without_a_token_is_unknown_and_apply_is_exit_3() {
    let api = start(&cf_routes());
    let d = dir(CF_TOML);
    let envs = vec![("HOIST_CLOUDFLARE_API", api.url())];
    let r = run(&["check"], &d, &envs, 0);
    assert_eq!(item(&r, "zone", "example.dev")["action"], "unknown");
    run(&["apply", "--yes"], &d, &envs, 3);
    assert!(api.requests().is_empty());
}

const NPM_TOML: &str = r#"[npm]
scope = "acme"
packages = ["@acme/tool"]
[npm.trusted_publisher]
repository = "acme/tool"
workflow = "release.yml"
environment = "release"
"#;

const PKG: &str = "/@acme%2Ftool";
const TRUST: &str = "/-/package/@acme%2Ftool/trust";
const TRUST_OK: &str =
    r#"[{"type":"github","repository":"acme/tool","file":"release.yml","environment":"release"}]"#;

fn npm_env(api: &FakeApi) -> Vec<(&'static str, String)> {
    vec![("HOIST_NPM_REGISTRY", api.url())]
}

#[test]
fn npm_package_and_scope_ok_trust_unknown_without_a_token_and_never_requested() {
    let api = start(&[
        (&format!("GET {PKG}"), 200, "{}"),
        (&format!("GET {TRUST}"), 200, TRUST_OK),
    ]);
    let d = dir(NPM_TOML);
    let r = run(&["check"], &d, &npm_env(&api), 0);
    api.assert_only_get();
    assert_eq!(item(&r, "scope", "@acme")["action"], "ok");
    assert_eq!(item(&r, "package", "@acme/tool")["action"], "ok");
    assert_eq!(
        item(&r, "trusted_publisher", "@acme/tool")["action"],
        "unknown"
    );
    assert!(api.requests().iter().all(|q| q.path != TRUST));
    assert!(api.requests().iter().all(|q| q.auth.is_none()));
}

#[test]
fn npm_trust_match_is_ok_with_a_token_and_a_mismatch_is_pending_with_the_command() {
    let api = start(&[
        (&format!("GET {PKG}"), 200, "{}"),
        (&format!("GET {TRUST}"), 200, TRUST_OK),
    ]);
    let d = dir(NPM_TOML);
    let mut envs = npm_env(&api);
    envs.push(("NPM_TOKEN", "npm-secret-tok".into()));
    let r = run(&["check"], &d, &envs, 0);
    assert_eq!(item(&r, "trusted_publisher", "@acme/tool")["action"], "ok");
    let t = api
        .requests()
        .into_iter()
        .find(|q| q.path == TRUST)
        .unwrap();
    assert_eq!(t.auth.as_deref(), Some("Bearer npm-secret-tok"));

    let api = start(&[
        (&format!("GET {PKG}"), 200, "{}"),
        (&format!("GET {TRUST}"), 200, "[]"),
    ]);
    let mut envs = npm_env(&api);
    envs.push(("NPM_TOKEN", "npm-secret-tok".into()));
    let r = run(&["check"], &d, &envs, 2);
    assert_eq!(
        item(&r, "trusted_publisher", "@acme/tool")["action"],
        "pending"
    );
    let step = r["human_steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "npm:trusted_publisher:@acme/tool")
        .expect("trust step");
    let title = step["title"].as_str().unwrap();
    assert!(
        title.contains(
            "npm trust github @acme/tool --file release.yml --repo acme/tool --env release"
        ),
        "{title}"
    );
}

#[test]
fn npm_trust_401_is_unknown_not_exit_3() {
    let api = start(&[
        (&format!("GET {PKG}"), 200, "{}"),
        (&format!("GET {TRUST}"), 401, "{}"),
    ]);
    let d = dir(NPM_TOML);
    let mut envs = npm_env(&api);
    envs.push(("NPM_TOKEN", "npm-secret-tok".into()));
    let r = run(&["check"], &d, &envs, 0);
    assert_eq!(
        item(&r, "trusted_publisher", "@acme/tool")["action"],
        "unknown"
    );
}

#[test]
fn npm_missing_package_is_pending_exit_2_with_bootstrap_and_scope_steps() {
    let api = start(&[(&format!("GET {PKG}"), 404, "{}")]);
    let d = dir("[npm]\nscope = \"acme\"\npackages = [\"@acme/tool\"]\n");
    let r = run(&["check"], &d, &npm_env(&api), 2);
    assert_eq!(item(&r, "package", "@acme/tool")["action"], "pending");
    assert_eq!(item(&r, "scope", "@acme")["action"], "unknown");
    let ids: Vec<&str> = r["human_steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"npm:package:@acme/tool"), "{ids:?}");
    assert!(ids.contains(&"npm:scope:@acme"), "{ids:?}");
    let t = r["human_steps"][1]["title"].as_str().unwrap().to_string()
        + r["human_steps"][0]["title"].as_str().unwrap();
    assert!(t.contains("hoist release bootstrap"));
}

const CRATES_TOML: &str = r#"[crates]
crates = ["acme-tool"]
owners = ["github:acme:owners", "someone"]
[crates.trusted_publisher]
repository = "acme/tool"
workflow = "release.yml"
environment = "release"
"#;

const OWNERS: &str = "/api/v1/crates/acme-tool/owners";
const CFG: &str = "/api/v1/trusted_publishing/github_configs";

fn crates_env(api: &FakeApi) -> Vec<(&'static str, String)> {
    vec![("HOIST_CRATES_API", api.url())]
}

#[test]
fn crate_owners_ok_and_missing_owner_is_pending_with_cargo_owner_step() {
    let api = start(&[(
        &format!("GET {OWNERS}"),
        200,
        r#"{"users":[{"login":"github:acme:owners"}]}"#,
    )]);
    let d = dir(CRATES_TOML);
    let r = run(&["check"], &d, &crates_env(&api), 2);
    assert_eq!(
        item(&r, "crate_owner", "acme-tool/github:acme:owners")["action"],
        "ok"
    );
    assert_eq!(
        item(&r, "crate_owner", "acme-tool/someone")["action"],
        "pending"
    );
    assert_eq!(
        item(&r, "trusted_publisher", "acme-tool")["action"],
        "unknown"
    );
    let step = r["human_steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "crates:crate_owner:acme-tool/someone")
        .expect("owner step");
    assert!(
        step["title"]
            .as_str()
            .unwrap()
            .contains("cargo owner --add someone acme-tool")
    );
    api.assert_only_get();
    assert!(api.requests().iter().all(|q| !q.path.starts_with(CFG)));
}

#[test]
fn missing_crate_is_pending_with_a_bootstrap_step() {
    let api = start(&[(&format!("GET {OWNERS}"), 404, "{}")]);
    let d = dir(CRATES_TOML);
    let r = run(&["check"], &d, &crates_env(&api), 2);
    assert_eq!(
        item(&r, "crate_owner", "acme-tool/someone")["action"],
        "pending"
    );
    assert!(r["human_steps"].as_array().unwrap().iter().any(|s| {
        s["id"] == "crates:crate:acme-tool"
            && s["title"]
                .as_str()
                .unwrap()
                .contains("hoist release bootstrap")
    }));
}

#[test]
fn crates_trust_401_is_unknown_and_the_token_is_sent_without_bearer() {
    let api = start(&[
        (
            &format!("GET {OWNERS}"),
            200,
            r#"{"users":[{"login":"someone"},{"login":"github:acme:owners"}]}"#,
        ),
        (&format!("GET {CFG}"), 401, "{}"),
    ]);
    let d = dir(CRATES_TOML);
    let mut envs = crates_env(&api);
    envs.push(("CARGO_REGISTRY_TOKEN", "cio-secret-tok".into()));
    let r = run(&["check"], &d, &envs, 0);
    assert_eq!(
        item(&r, "trusted_publisher", "acme-tool")["action"],
        "unknown"
    );
    let t = api
        .requests()
        .into_iter()
        .find(|q| q.path == CFG)
        .expect("trust request");
    assert_eq!(t.auth.as_deref(), Some("cio-secret-tok"));
    assert!(t.url.contains("crate=acme-tool"));
    let o = api
        .requests()
        .into_iter()
        .find(|q| q.path == OWNERS)
        .unwrap();
    assert_eq!(o.auth, None);
}

#[test]
fn crates_trust_match_ok_and_no_match_pending_with_the_settings_url() {
    let good = r#"{"github_configs":[{"repository_owner":"acme","repository_name":"tool","workflow_filename":"release.yml","environment":"release"}]}"#;
    let owners = r#"{"users":[{"login":"someone"},{"login":"github:acme:owners"}]}"#;
    let d = dir(CRATES_TOML);
    let api = start(&[
        (&format!("GET {OWNERS}"), 200, owners),
        (&format!("GET {CFG}"), 200, good),
    ]);
    let mut envs = crates_env(&api);
    envs.push(("CARGO_REGISTRY_TOKEN", "cio-secret-tok".into()));
    let r = run(&["check"], &d, &envs, 0);
    assert_eq!(item(&r, "trusted_publisher", "acme-tool")["action"], "ok");

    let api = start(&[
        (&format!("GET {OWNERS}"), 200, owners),
        (&format!("GET {CFG}"), 200, r#"{"github_configs":[]}"#),
    ]);
    let mut envs = crates_env(&api);
    envs.push(("CARGO_REGISTRY_TOKEN", "cio-secret-tok".into()));
    let r = run(&["check"], &d, &envs, 2);
    let step = r["human_steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "crates:trusted_publisher:acme-tool")
        .unwrap();
    assert_eq!(step["url"], "https://crates.io/crates/acme-tool/settings");
}

const SC_TOML: &str = r#"[search_console]
[[search_console.sites]]
domain = "example.dev"
zone = "example.dev"
sitemaps = ["https://example.dev/sitemap.xml"]
"#;

const SITE: &str = "/wm/sites/sc-domain%3Aexample.dev";

fn sc_env(bridge: &FakeApi, cf: Option<&FakeApi>) -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("HOIST_SEARCH_CONSOLE_BRIDGE", bridge.url()),
        ("HOIST_SEARCH_CONSOLE_TOKEN", "sc-secret-tok".into()),
    ];
    if let Some(cf) = cf {
        v.push(("HOIST_CLOUDFLARE_API", cf.url()));
        v.push(("CLOUDFLARE_API_TOKEN", "cf-test-token".into()));
    }
    v
}

fn sc_ok_routes() -> Vec<(String, u16, String)> {
    let r = |k: &str, b: &str| (k.to_string(), 200, b.to_string());
    vec![
        r(
            "GET /sv/webResource",
            r#"{"items":[{"id":"dns%3A%2F%2Fexample.dev","site":{"type":"INET_DOMAIN","identifier":"example.dev"}}]}"#,
        ),
        r(
            "GET /wm/sites",
            r#"{"siteEntry":[{"siteUrl":"sc-domain:example.dev","permissionLevel":"siteOwner"}]}"#,
        ),
        r(
            &format!("GET {SITE}/sitemaps"),
            r#"{"sitemap":[{"path":"https://example.dev/sitemap.xml"}]}"#,
        ),
    ]
}

fn api_of(routes: &[(String, u16, String)]) -> FakeApi {
    let v: Vec<(&str, u16, &str)> = routes
        .iter()
        .map(|(k, s, b)| (k.as_str(), *s, b.as_str()))
        .collect();
    start(&v)
}

#[test]
fn search_console_everything_ok_is_exit_0_and_get_only() {
    let bridge = api_of(&sc_ok_routes());
    let d = dir(SC_TOML);
    let r = run(&["check"], &d, &sc_env(&bridge, None), 0);
    bridge.assert_only_get();
    assert_eq!(item(&r, "verification", "example.dev")["action"], "ok");
    assert_eq!(
        item(&r, "property", "sc-domain:example.dev")["action"],
        "ok"
    );
    assert_eq!(
        item(&r, "sitemap", "https://example.dev/sitemap.xml")["action"],
        "ok"
    );
    let auth = bridge.requests()[0].auth.clone();
    assert_eq!(auth.as_deref(), Some("Bearer sc-secret-tok"));
}

fn sc_fresh_routes(insert_status: u16) -> Vec<(String, u16, String)> {
    let r = |k: &str, s: u16, b: &str| (k.to_string(), s, b.to_string());
    vec![
        r("GET /sv/webResource", 200, r#"{"items":[]}"#),
        r("GET /wm/sites", 200, r#"{"siteEntry":[]}"#),
        r(
            "POST /sv/token",
            200,
            r#"{"method":"DNS_TXT","token":"google-site-verification=abc"}"#,
        ),
        r(
            "POST /sv/webResource",
            insert_status,
            r#"{"error":{"message":"The necessary verification token could not be found"}}"#,
        ),
        r(&format!("PUT {SITE}"), 204, ""),
        r(
            &format!("PUT {SITE}/sitemaps/https%3A%2F%2Fexample.dev%2Fsitemap.xml"),
            204,
            "",
        ),
    ]
}

fn cf_for_sc() -> FakeApi {
    start(&[
        (
            "GET /zones?name=example.dev",
            200,
            r#"{"success":true,"result":[{"id":"z1","name":"example.dev"}]}"#,
        ),
        (
            "GET /zones/z1/dns_records",
            200,
            r#"{"success":true,"result":[],"result_info":{"page":1,"total_pages":1}}"#,
        ),
        (
            "POST /zones/z1/dns_records",
            200,
            r#"{"success":true,"result":{"id":"r1"}}"#,
        ),
    ])
}

#[test]
fn search_console_plan_is_get_only_and_apply_runs_the_full_flow_in_order() {
    let bridge = api_of(&sc_fresh_routes(200));
    let cf = cf_for_sc();
    let d = dir(SC_TOML);
    let envs = sc_env(&bridge, Some(&cf));
    let r = run(&["plan"], &d, &envs, 0);
    bridge.assert_only_get();
    cf.assert_only_get();
    assert_eq!(item(&r, "verification", "example.dev")["action"], "create");
    assert_eq!(
        item(&r, "property", "sc-domain:example.dev")["action"],
        "create"
    );
    // The fake keeps answering "not there", so the re-read shows creates: exit 1.
    run(&["apply", "--yes"], &d, &envs, 1);
    let seq: Vec<String> = bridge
        .requests()
        .iter()
        .filter(|q| q.method != "GET")
        .map(|q| format!("{} {}", q.method, q.url))
        .collect();
    assert_eq!(
        seq,
        [
            "POST /sv/token",
            "POST /sv/webResource?verificationMethod=DNS_TXT",
            "PUT /wm/sites/sc-domain%3Aexample.dev",
            "PUT /wm/sites/sc-domain%3Aexample.dev/sitemaps/https%3A%2F%2Fexample.dev%2Fsitemap.xml",
        ]
    );
    let breq = bridge.requests();
    let insert = breq
        .iter()
        .find(|q| q.path == "/sv/webResource" && q.method == "POST")
        .unwrap();
    let body: Value = serde_json::from_str(insert.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["site"]["type"], "INET_DOMAIN");
    assert_eq!(body["site"]["identifier"], "example.dev");
    let txt = cf
        .requests()
        .into_iter()
        .find(|q| q.method == "POST")
        .expect("TXT record POST");
    assert_eq!(txt.path, "/zones/z1/dns_records");
    let body: Value = serde_json::from_str(txt.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["type"], "TXT");
    assert_eq!(body["name"], "example.dev");
    assert_eq!(body["content"], "google-site-verification=abc");
}

#[test]
fn search_console_insert_refused_is_pending_exit_2_and_skips_the_property() {
    let bridge = api_of(&sc_fresh_routes(400));
    let cf = cf_for_sc();
    let d = dir(SC_TOML);
    let r = run(&["apply", "--yes"], &d, &sc_env(&bridge, Some(&cf)), 2);
    assert_eq!(item(&r, "verification", "example.dev")["action"], "pending");
    assert!(r["human_steps"].as_array().unwrap().iter().any(|s| {
        s["id"] == "search_console:verification:example.dev"
            && s["title"]
                .as_str()
                .unwrap()
                .contains("re-run `hoist org apply`")
    }));
    assert!(bridge.requests().iter().all(|q| q.method != "PUT"));
}

#[test]
fn search_console_without_a_token_is_unknown_in_check_and_exit_3_in_apply() {
    let bridge = api_of(&sc_ok_routes());
    let d = dir(SC_TOML);
    let envs = vec![("HOIST_SEARCH_CONSOLE_BRIDGE", bridge.url())];
    let r = run(&["check"], &d, &envs, 0);
    assert_eq!(item(&r, "verification", "example.dev")["action"], "unknown");
    assert!(
        item(&r, "verification", "example.dev")["reason"]
            .as_str()
            .unwrap()
            .contains("HOIST_SEARCH_CONSOLE_TOKEN")
    );
    run(&["apply", "--yes"], &d, &envs, 3);
    assert!(bridge.requests().is_empty());
}

#[test]
fn search_console_without_a_bridge_is_unknown_and_apply_is_exit_3() {
    let d = dir(SC_TOML);
    let envs = vec![("HOIST_SEARCH_CONSOLE_TOKEN", "sc-secret-tok".to_string())];
    // hoist() points the bridge at a closed port; remove it for this run.
    let mut c = hoist();
    c.env_remove("HOIST_SEARCH_CONSOLE_BRIDGE");
    for (k, v) in &envs {
        c.env(k, v);
    }
    c.current_dir(d.path())
        .args(["--json", "org", "check"])
        .assert()
        .code(0);
    let mut c = hoist();
    c.env_remove("HOIST_SEARCH_CONSOLE_BRIDGE");
    c.env("HOIST_SEARCH_CONSOLE_TOKEN", "sc-secret-tok");
    c.current_dir(d.path())
        .args(["org", "apply", "--yes"])
        .assert()
        .code(3);
}
