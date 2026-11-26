//! `hoist org` `[fly]` (spec §16.7) and the Search Console bridge deploy (§16.8), on fakes only.

mod common;

use common::{FakeApi, RouteSpec, hoist};
use serde_json::Value;
use tempfile::TempDir;

const FLY_TOML: &str = r#"[fly]
org = "acme"
[[fly.apps]]
name = "acme-api"
region = "fra"
secrets = ["DATABASE_URL"]
certificates = ["api.example.dev"]
"#;

fn dir(toml: &str) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("org.toml"), toml).unwrap();
    d
}

fn api(routes: &[(&str, u16, &str)]) -> FakeApi {
    let specs: Vec<RouteSpec> = routes
        .iter()
        .map(|(k, s, b)| (*k, *s, *b, &[][..]))
        .collect();
    FakeApi::start(&specs)
}

const SECRET_TOKENS: [&str; 6] = [
    "fly-secret-tok",
    "cf-test-token",
    "sc-secret-tok",
    "bridge-token-0123456789abcdef0123456789abcdef",
    "PRIVATE-KEY-MATERIAL",
    "hunter2",
];

fn run(args: &[&str], d: &TempDir, envs: &[(&str, String)], code: i32) -> Value {
    let mut c = hoist();
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c
        .current_dir(d.path())
        .args(["--json", "-v", "org"])
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
    for secret in SECRET_TOKENS {
        assert!(!all.contains(secret), "secret leaked: {all}");
    }
    serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("one JSON object: {all}"))
}

fn item<'a>(r: &'a Value, kind: &str, key: &str) -> &'a Value {
    r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == kind && i["key"] == key)
        .unwrap_or_else(|| panic!("no item {kind} {key} in {r}"))
}

fn fly_env(api: &FakeApi) -> Vec<(&'static str, String)> {
    vec![
        ("HOIST_FLY_API", api.url()),
        ("FLY_API_TOKEN", "fly-secret-tok".into()),
    ]
}

const APPS: &str = r#"{"total_apps":1,"apps":[{"id":"x","name":"acme-api","machine_count":1}]}"#;

fn fly_ok_routes() -> Vec<(&'static str, u16, &'static str)> {
    vec![
        ("GET /apps?org_slug=acme", 200, APPS),
        (
            "GET /apps/acme-api/machines",
            200,
            r#"[{"id":"m1","region":"fra"}]"#,
        ),
        (
            "GET /apps/acme-api/secrets",
            200,
            r#"{"secrets":[{"name":"DATABASE_URL","digest":"d"}]}"#,
        ),
        (
            "GET /apps/acme-api/certificates",
            200,
            r#"{"certificates":[{"hostname":"api.example.dev","status":"Ready"}],"total_count":1}"#,
        ),
    ]
}

#[test]
fn fly_all_ok_is_exit_0_get_only_with_a_bearer_token() {
    let api = api(&fly_ok_routes());
    let d = dir(FLY_TOML);
    let r = run(&["check"], &d, &fly_env(&api), 0);
    api.assert_only_get();
    assert_eq!(item(&r, "org", "acme")["action"], "ok");
    assert_eq!(item(&r, "app", "acme-api")["action"], "ok");
    assert_eq!(item(&r, "region", "acme-api")["action"], "ok");
    assert_eq!(item(&r, "secret", "acme-api/DATABASE_URL")["action"], "ok");
    assert_eq!(
        item(&r, "certificate", "acme-api/api.example.dev")["action"],
        "ok"
    );
    assert_eq!(item(&r, "org", "acme")["area"], "fly");
    let auth = api.requests()[0].auth.clone();
    assert_eq!(auth.as_deref(), Some("Bearer fly-secret-tok"));
}

#[test]
fn a_proxy_injected_dummy_token_is_sent_as_is() {
    let api = api(&fly_ok_routes());
    let d = dir(FLY_TOML);
    let envs = vec![
        ("HOIST_FLY_API", api.url()),
        ("FLY_API_TOKEN", "proxy-injected".to_string()),
    ];
    run(&["plan"], &d, &envs, 0);
    api.assert_only_get();
    assert!(
        api.requests()
            .iter()
            .all(|q| q.auth.as_deref() == Some("Bearer proxy-injected"))
    );
}

#[test]
fn missing_secret_region_and_pending_certificate_are_pending_with_steps() {
    let api = api(&[
        ("GET /apps?org_slug=acme", 200, APPS),
        (
            "GET /apps/acme-api/machines",
            200,
            r#"[{"id":"m1","region":"ams"}]"#,
        ),
        ("GET /apps/acme-api/secrets", 200, r#"{"secrets":[]}"#),
        (
            "GET /apps/acme-api/certificates",
            200,
            r#"{"certificates":[{"hostname":"api.example.dev","status":"Awaiting configuration"}]}"#,
        ),
    ]);
    let d = dir(FLY_TOML);
    let r = run(&["check"], &d, &fly_env(&api), 2);
    api.assert_only_get();
    assert_eq!(item(&r, "region", "acme-api")["action"], "pending");
    assert_eq!(
        item(&r, "secret", "acme-api/DATABASE_URL")["action"],
        "pending"
    );
    assert_eq!(
        item(&r, "certificate", "acme-api/api.example.dev")["action"],
        "pending"
    );
    let ids: Vec<&str> = r["human_steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"fly:secret:acme-api/DATABASE_URL"), "{ids:?}");
    assert!(ids.contains(&"fly:region:acme-api"));
    assert!(ids.contains(&"fly:certificate:acme-api/api.example.dev"));
}

#[test]
fn missing_org_is_pending_with_a_step() {
    let api = api(&[("GET /apps?org_slug=acme", 404, r#"{"error":"not found"}"#)]);
    let d = dir(FLY_TOML);
    let r = run(&["check"], &d, &fly_env(&api), 2);
    assert_eq!(item(&r, "org", "acme")["action"], "pending");
    assert_eq!(r["human_steps"][0]["id"], "fly:org:acme");
}

#[test]
fn a_401_or_403_is_unknown_not_an_error() {
    for status in [401, 403] {
        let api = api(&[("GET /apps?org_slug=acme", status, r#"{"error":"no"}"#)]);
        let d = dir(FLY_TOML);
        let r = run(&["check"], &d, &fly_env(&api), 0);
        assert_eq!(item(&r, "org", "acme")["action"], "unknown", "{status}");
        assert_eq!(item(&r, "app", "acme-api")["action"], "unknown");
        assert_eq!(
            item(&r, "certificate", "acme-api/api.example.dev")["action"],
            "unknown"
        );
        api.assert_only_get();
    }
}

#[test]
fn no_token_is_unknown_with_a_confirm_step_and_apply_is_exit_3() {
    let api = api(&fly_ok_routes());
    let d = dir(FLY_TOML);
    let envs = vec![("HOIST_FLY_API", api.url())];
    let r = run(&["check"], &d, &envs, 0);
    assert_eq!(item(&r, "org", "acme")["action"], "unknown");
    assert_eq!(r["human_steps"][0]["id"], "fly:token:FLY_API_TOKEN");
    assert_eq!(r["human_steps"][0]["status"], "confirm");
    run(&["apply", "--yes"], &d, &envs, 3);
    assert!(api.requests().is_empty());
}

#[test]
fn apply_creates_the_missing_app_then_its_certificate() {
    let api = api(&[
        ("GET /apps?org_slug=acme", 200, r#"{"apps":[]}"#),
        ("POST /apps", 201, r#"{"id":"x"}"#),
        (
            "POST /apps/acme-api/certificates/acme",
            200,
            r#"{"hostname":"api.example.dev"}"#,
        ),
    ]);
    let d = dir(FLY_TOML);
    let r = run(&["plan"], &d, &fly_env(&api), 0);
    api.assert_only_get();
    assert_eq!(item(&r, "app", "acme-api")["action"], "create");
    assert_eq!(
        item(&r, "certificate", "acme-api/api.example.dev")["action"],
        "create"
    );
    assert_eq!(
        item(&r, "secret", "acme-api/DATABASE_URL")["action"],
        "unknown"
    );
    // The fake keeps saying "no apps", so the re-read still shows creates: exit 1.
    run(&["apply", "--yes"], &d, &fly_env(&api), 1);
    let posts: Vec<_> = api
        .requests()
        .into_iter()
        .filter(|q| q.method == "POST")
        .collect();
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].path, "/apps");
    let b: Value = serde_json::from_str(posts[0].body.as_deref().unwrap()).unwrap();
    assert_eq!(b["app_name"], "acme-api");
    assert_eq!(b["org_slug"], "acme");
    assert_eq!(posts[1].path, "/apps/acme-api/certificates/acme");
    let b: Value = serde_json::from_str(posts[1].body.as_deref().unwrap()).unwrap();
    assert_eq!(b["hostname"], "api.example.dev");
}

#[test]
fn apply_updates_a_missing_certificate_of_an_existing_app_and_reports_a_refusal() {
    let mut routes = fly_ok_routes();
    routes.retain(|r| !r.0.ends_with("/certificates"));
    routes.push((
        "GET /apps/acme-api/certificates",
        200,
        r#"{"certificates":[]}"#,
    ));
    routes.push((
        "POST /apps/acme-api/certificates/acme",
        422,
        r#"{"error":"bad hostname"}"#,
    ));
    let api = api(&routes);
    let d = dir(FLY_TOML);
    let r = run(&["apply", "--yes"], &d, &fly_env(&api), 1);
    let failed = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["kind"] == "write" && i["reason"].as_str().unwrap().contains("422"));
    assert!(failed, "{r}");
}

#[test]
fn fly_validation_errors_are_exit_3() {
    for bad in [
        "[fly]\norg = \"\"\n",
        "[fly]\norg = \"a\"\n[[fly.apps]]\nname = \"Bad_Name\"\n",
        "[fly]\norg = \"a\"\n[[fly.apps]]\nname = \"x\"\nregion = \"FRA\"\n",
        "[fly]\norg = \"a\"\n[[fly.apps]]\nname = \"x\"\nsecrets = [\"A=B\"]\n",
        "[fly]\norg = \"a\"\n[[fly.apps]]\nname = \"x\"\ncertificates = [\"nodot\"]\n",
        "[fly]\norg = \"a\"\n[[fly.apps]]\nname = \"x\"\n[[fly.apps]]\nname = \"x\"\n",
        "[fly]\norg = \"a\"\ntoken = \"x\"\n",
    ] {
        run(&["plan"], &dir(bad), &[], 3);
    }
}

// ---- Search Console bridge deploy (spec §16.8) ----

const BRIDGE_TOML: &str = r#"[cloudflare]
account_id = "acct"
[search_console]
bridge = "https://sc-bridge.example.workers.dev"
[search_console.worker]
name = "sc-bridge"
[[search_console.sites]]
domain = "example.dev"
zone = "example.dev"
"#;

const SCRIPT: &str = "/accounts/acct/workers/scripts/sc-bridge";

fn cf_env(api: &FakeApi) -> Vec<(&'static str, String)> {
    vec![
        ("HOIST_CLOUDFLARE_API", api.url()),
        ("CLOUDFLARE_API_TOKEN", "cf-test-token".into()),
    ]
}

fn bridge_env(cf: &FakeApi) -> Vec<(&'static str, String)> {
    let mut v = cf_env(cf);
    v.push(("HOIST_SEARCH_CONSOLE_TOKEN", "sc-secret-tok".into()));
    v
}

const SA_KEY: &str = r#"{"client_email":"sa@p.iam","private_key":"PRIVATE-KEY-MATERIAL"}"#;

fn value_env(mut v: Vec<(&'static str, String)>) -> Vec<(&'static str, String)> {
    v.push((
        "HOIST_BRIDGE_TOKEN",
        "bridge-token-0123456789abcdef0123456789abcdef".into(),
    ));
    v.push(("HOIST_BRIDGE_SA_KEY", SA_KEY.into()));
    v
}

fn fresh_cf() -> FakeApi {
    api(&[
        (
            "PUT /accounts/acct/workers/scripts/sc-bridge",
            200,
            r#"{"success":true,"result":{}}"#,
        ),
        (
            "POST /accounts/acct/workers/scripts/sc-bridge/subdomain",
            200,
            r#"{"success":true,"result":{"enabled":true}}"#,
        ),
        (
            "PUT /accounts/acct/workers/scripts/sc-bridge/secrets",
            200,
            r#"{"success":true,"result":{}}"#,
        ),
    ])
}

#[test]
fn bridge_plan_is_get_only_and_sites_wait_for_the_worker() {
    let cf = fresh_cf();
    let d = dir(BRIDGE_TOML);
    let r = run(&["plan"], &d, &value_env(bridge_env(&cf)), 0);
    cf.assert_only_get();
    assert_eq!(item(&r, "worker", "sc-bridge")["action"], "create");
    assert_eq!(
        item(&r, "worker_secret", "sc-bridge/SA_KEY")["action"],
        "create"
    );
    assert_eq!(item(&r, "verification", "example.dev")["action"], "unknown");
    assert!(
        item(&r, "verification", "example.dev")["reason"]
            .as_str()
            .unwrap()
            .contains("bridge Worker is not deployed")
    );
}

#[test]
fn bridge_apply_deploys_the_worker_and_sets_both_secrets_without_leaking_them() {
    let cf = fresh_cf();
    let d = dir(BRIDGE_TOML);
    // The fake keeps answering "no worker", so the re-read still shows creates: exit 1.
    run(&["apply", "--yes"], &d, &value_env(bridge_env(&cf)), 1);
    let reqs = cf.requests();
    let put = reqs
        .iter()
        .find(|q| q.method == "PUT" && q.path == SCRIPT)
        .expect("script upload");
    assert!(
        put.content_type
            .as_deref()
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    let body = String::from_utf8_lossy(&put.raw);
    assert!(body.contains(r#""main_module":"worker.js""#), "{body}");
    assert!(body.contains("BRIDGE_TOKEN"), "worker source is uploaded");
    assert!(
        reqs.iter()
            .any(|q| q.method == "POST" && q.path == format!("{SCRIPT}/subdomain"))
    );
    let secrets: Vec<Value> = reqs
        .iter()
        .filter(|q| q.method == "PUT" && q.path == format!("{SCRIPT}/secrets"))
        .map(|q| serde_json::from_str(q.body.as_deref().unwrap()).unwrap())
        .collect();
    assert_eq!(secrets.len(), 2);
    let sa = secrets.iter().find(|s| s["name"] == "SA_KEY").unwrap();
    assert_eq!(sa["type"], "secret_text");
    assert!(
        sa["text"]
            .as_str()
            .unwrap()
            .contains("PRIVATE-KEY-MATERIAL")
    );
    let order: Vec<&str> = reqs
        .iter()
        .filter(|q| q.method != "GET")
        .map(|q| q.path.as_str())
        .collect();
    assert_eq!(order[0], SCRIPT, "deploy first: {order:?}");
}

#[test]
fn bridge_sa_key_from_a_file_and_a_bad_key_is_exit_3_before_any_write() {
    let cf = fresh_cf();
    let d = dir(BRIDGE_TOML);
    let key = d.path().join("key.json");
    std::fs::write(&key, SA_KEY).unwrap();
    let mut envs = bridge_env(&cf);
    envs.push((
        "HOIST_BRIDGE_TOKEN",
        "bridge-token-0123456789abcdef0123456789abcdef".into(),
    ));
    envs.push(("HOIST_BRIDGE_SA_KEY_FILE", key.display().to_string()));
    run(&["apply", "--yes"], &d, &envs, 1);
    assert!(
        cf.requests()
            .iter()
            .any(|q| q.path == format!("{SCRIPT}/secrets"))
    );

    let cf2 = fresh_cf();
    let mut bad = bridge_env(&cf2);
    bad.push((
        "HOIST_BRIDGE_TOKEN",
        "bridge-token-0123456789abcdef0123456789abcdef".into(),
    ));
    bad.push(("HOIST_BRIDGE_SA_KEY", "hunter2".into()));
    run(&["apply", "--yes"], &d, &bad, 3);
    assert!(cf2.requests().iter().all(|q| q.method == "GET"));
    let mut short = bridge_env(&cf2);
    short.push(("HOIST_BRIDGE_TOKEN", "hunter2".into()));
    short.push(("HOIST_BRIDGE_SA_KEY", SA_KEY.into()));
    run(&["apply", "--yes"], &d, &short, 3);
    assert!(cf2.requests().iter().all(|q| q.method == "GET"));
}

#[test]
fn bridge_without_value_sources_is_pending_with_one_step_per_secret() {
    let cf = api(&[
        (
            "GET /accounts/acct/workers/scripts/sc-bridge/settings",
            200,
            r#"{"result":{}}"#,
        ),
        (
            "GET /accounts/acct/workers/scripts/sc-bridge/secrets",
            200,
            r#"{"result":[]}"#,
        ),
    ]);
    let d = dir(BRIDGE_TOML);
    let r = run(&["check"], &d, &bridge_env(&cf), 2);
    cf.assert_only_get();
    assert_eq!(item(&r, "worker", "sc-bridge")["action"], "ok");
    assert_eq!(
        item(&r, "worker_secret", "sc-bridge/SA_KEY")["action"],
        "pending"
    );
    let ids: Vec<&str> = r["human_steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&"search_console:worker_secret:sc-bridge/SA_KEY"),
        "{ids:?}"
    );
    assert!(ids.contains(&"search_console:worker_secret:sc-bridge/BRIDGE_TOKEN"));
}

#[test]
fn a_ready_worker_lets_the_sites_read_through_the_bridge() {
    let cf = api(&[
        (
            "GET /accounts/acct/workers/scripts/sc-bridge/settings",
            200,
            r#"{"result":{}}"#,
        ),
        (
            "GET /accounts/acct/workers/scripts/sc-bridge/secrets",
            200,
            r#"{"result":[{"name":"SA_KEY","type":"secret_text"},{"name":"BRIDGE_TOKEN","type":"secret_text"}]}"#,
        ),
    ]);
    let bridge = api(&[
        (
            "GET /sv/webResource",
            200,
            r#"{"items":[{"id":"dns%3A%2F%2Fexample.dev","site":{"type":"INET_DOMAIN","identifier":"example.dev"}}]}"#,
        ),
        (
            "GET /wm/sites",
            200,
            r#"{"siteEntry":[{"siteUrl":"sc-domain:example.dev","permissionLevel":"siteOwner"}]}"#,
        ),
    ]);
    let d = dir(BRIDGE_TOML);
    let mut envs = bridge_env(&cf);
    envs.push(("HOIST_SEARCH_CONSOLE_BRIDGE", bridge.url()));
    let r = run(&["check"], &d, &envs, 0);
    cf.assert_only_get();
    bridge.assert_only_get();
    assert_eq!(item(&r, "worker", "sc-bridge")["action"], "ok");
    assert_eq!(item(&r, "verification", "example.dev")["action"], "ok");
}

#[test]
fn worker_config_errors_are_exit_3() {
    run(
        &["plan"],
        &dir("[search_console]\n[search_console.worker]\nname = \"x\"\n"),
        &[],
        3,
    );
    run(
        &["plan"],
        &dir(
            "[cloudflare]\naccount_id=\"a\"\n[search_console]\n[search_console.worker]\nname = \"Bad Name\"\n",
        ),
        &[],
        3,
    );
}
