//! `hoist system check --live` for the Fly, npm, crates.io and Google probes (spec §17.5).

mod common;

use common::{FakeApi, hoist};
use serde_json::Value;
use std::fs;
use std::path::Path;

const NOW: &str = "2026-11-27T12:00:00Z";
const TOKEN: &str = "canary-live-token-4f8a1c";

fn workflow(key: &str) -> String {
    format!(
        "name: deploy\non: push\njobs:\n  deploy:\n    runs-on: ubuntu-latest\n    environment: site\n    steps:\n      - run: echo deploy\n        env:\n          {key}: ${{{{ secrets.{key} }}}}\n"
    )
}

/// A system.toml with one credential and the extra lines given.
fn system(provider: &str, key: &str, role: &str, extra: &str) -> String {
    format!(
        r#"version = 1

[system]
name = "acme/app"
owner = "alice"
description = "Test system"

[[provider]]
id = "{provider}"
capability = "registry"

[[credential]]
id = "cred"
provider = "{provider}"
kind = "api_token"
role = "{role}"
purpose = "Test credential"
owner = "alice"
created_at = "2026-11-01"
expires_at = "2027-01-20"
keyless_exempt = "no trusted publishing here"
storage = [{{ kind = "gh-env-secret", repo = "acme/app", env = "site", key = "{key}" }}]
consumers = [{{ workflow = "deploy.yml", job = "deploy", environment = "site" }}]
{extra}
"#
    )
}

fn repo(toml: &str, key: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir_in(common::tmpdir()).unwrap();
    let wf = dir.path().join(".github/workflows");
    fs::create_dir_all(&wf).unwrap();
    fs::write(wf.join("deploy.yml"), workflow(key)).unwrap();
    fs::write(dir.path().join("system.toml"), toml).unwrap();
    dir
}

/// Runs `system check --live --json`; asserts the exit code and that no token value shows up.
fn run(dir: &Path, envs: &[(&str, String)], code: i32) -> Value {
    let mut cmd = hoist();
    cmd.env("HOIST_NOW", NOW);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd
        .arg("-C")
        .arg(dir)
        .args(["system", "check", "--live", "--json", "-v"])
        .assert()
        .code(code)
        .get_output()
        .clone();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.contains("canary-"), "a token value leaked: {all}");
    serde_json::from_slice(&out.stdout).expect("one JSON object")
}

fn live(r: &Value) -> &Value {
    r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "live")
        .unwrap_or_else(|| panic!("no live item: {r}"))
}

fn expect(r: &Value, action: &str, contains: &str) {
    let l = live(r);
    assert_eq!(l["action"], action, "{r}");
    let text = l["reason"].as_str().or(l["detail"].as_str()).unwrap_or("");
    let whole = l.to_string();
    assert!(
        text.contains(contains) || whole.contains(contains),
        "want `{contains}` in {whole}"
    );
}

fn offline_clean(dir: &Path) {
    hoist()
        .env("HOIST_NOW", NOW)
        .arg("-C")
        .arg(dir)
        .args(["system", "check"])
        .assert()
        .code(0);
}

// ---------------------------------------------------------------- npm

const NPM_LIST: &str = r#"{"objects":[{"token":"npm_ab…","key":"keyabc123","name":"ci","cidr_whitelist":["10.0.0.0/8"],"readonly":false,"automation":true,"created":"2026-11-01T00:00:00.000Z","updated":"2026-11-02T00:00:00.000Z","expires":"2027-01-20T00:00:00.000Z"}],"urls":{}}"#;

fn npm_run(body: &str, status: u16, extra: &str, role: &str, token: bool, code: i32) -> Value {
    let api = FakeApi::start(&[("GET /-/npm/v1/tokens", status, body, &[])]);
    let d = repo(&system("npm", "NPM_TOKEN", role, extra), "NPM_TOKEN");
    offline_clean(d.path());
    let mut envs = vec![("HOIST_NPM_REGISTRY", api.url())];
    if token {
        envs.push(("NPM_TOKEN", TOKEN.to_string()));
    }
    let r = run(d.path(), &envs, code);
    api.assert_only_get();
    r
}

#[test]
fn npm_ok_names_prefix_created_readonly_and_cidr() {
    let r = npm_run(
        NPM_LIST,
        200,
        "provider_ref = \"keyabc123\"",
        "publish",
        true,
        0,
    );
    expect(&r, "ok", "token npm_ab");
    expect(&r, "ok", "created 2026-11-01");
    expect(&r, "ok", "readonly false");
    expect(&r, "ok", "10.0.0.0/8");
}

#[test]
fn npm_matches_a_ref_that_prefixes_the_masked_token() {
    let r = npm_run(
        NPM_LIST,
        200,
        "provider_ref = \"npm_abcdef\"",
        "publish",
        true,
        0,
    );
    expect(&r, "ok", "token npm_ab");
}

#[test]
fn npm_short_ref_never_matches_a_masked_prefix() {
    let r = npm_run(
        NPM_LIST,
        200,
        "provider_ref = \"npm_ab\"",
        "publish",
        true,
        1,
    );
    expect(&r, "drift", "not found");
}

#[test]
fn npm_missing_token_is_unknown_naming_the_env() {
    let r = npm_run(
        NPM_LIST,
        200,
        "provider_ref = \"keyabc123\"",
        "publish",
        false,
        0,
    );
    expect(&r, "unknown", "NPM_TOKEN");
}

#[test]
fn npm_storage_key_hoist_does_not_own_is_never_read() {
    // The value sits in env under a name npm does not own: hoist must not read it.
    let api = FakeApi::start(&[("GET /-/npm/v1/tokens", 200, NPM_LIST, &[])]);
    let d = repo(
        &system(
            "npm",
            "MY_NPM_SECRET",
            "publish",
            "provider_ref = \"keyabc123\"",
        ),
        "MY_NPM_SECRET",
    );
    let envs = vec![
        ("HOIST_NPM_REGISTRY", api.url()),
        ("MY_NPM_SECRET", TOKEN.to_string()),
    ];
    let r = run(d.path(), &envs, 0);
    expect(&r, "unknown", "MY_NPM_SECRET");
    expect(&r, "unknown", "NPM_TOKEN");
    assert_eq!(api.requests().len(), 0, "nothing may be sent");
}

#[test]
fn npm_no_provider_ref_is_unknown() {
    let r = npm_run(NPM_LIST, 200, "", "publish", true, 0);
    expect(&r, "unknown", "provider_ref");
}

#[test]
fn npm_token_not_listed_is_drift() {
    let r = npm_run(NPM_LIST, 200, "provider_ref = \"gone\"", "publish", true, 1);
    expect(&r, "drift", "not");
}

#[test]
fn npm_read_only_publish_token_is_drift() {
    let body = NPM_LIST.replace("\"readonly\":false", "\"readonly\":true");
    let r = npm_run(
        &body,
        200,
        "provider_ref = \"keyabc123\"",
        "publish",
        true,
        1,
    );
    expect(&r, "drift", "read-only");
}

#[test]
fn npm_writable_read_token_is_drift() {
    let r = npm_run(
        NPM_LIST,
        200,
        "provider_ref = \"keyabc123\"",
        "read",
        true,
        1,
    );
    expect(&r, "drift", "not read-only");
}

#[test]
fn npm_expiry_differs_is_drift() {
    let body = NPM_LIST.replace("2027-01-20", "2027-03-01");
    let r = npm_run(
        &body,
        200,
        "provider_ref = \"keyabc123\"",
        "publish",
        true,
        1,
    );
    expect(&r, "drift", "2027-03-01");
}

#[test]
fn npm_without_expires_skips_the_expiry_compare() {
    let body = NPM_LIST.replace(",\"expires\":\"2027-01-20T00:00:00.000Z\"", "");
    let r = npm_run(
        &body,
        200,
        "provider_ref = \"keyabc123\"",
        "publish",
        true,
        0,
    );
    expect(&r, "ok", "token npm_ab");
}

#[test]
fn npm_401_and_403_are_unknown() {
    for status in [401, 403] {
        let r = npm_run(
            "{}",
            status,
            "provider_ref = \"keyabc123\"",
            "publish",
            true,
            0,
        );
        expect(&r, "unknown", "NPM_TOKEN");
    }
}

// ---------------------------------------------------------------- crates

const CRATES_LIST: &str = r#"{"api_tokens":[{"id":42,"name":"publish","created_at":"2026-11-01T00:00:00Z","last_used_at":"2026-11-20T10:00:00Z","expired_at":"2027-01-20T00:00:00Z","crate_scopes":["keel-*"],"endpoint_scopes":["publish-update"]}]}"#;

fn crates_run(body: &str, status: u16, extra: &str, token: bool, code: i32) -> Value {
    let api = FakeApi::start(&[("GET /api/v1/me/tokens", status, body, &[])]);
    let d = repo(
        &system("crates", "CARGO_REGISTRY_TOKEN", "publish", extra),
        "CARGO_REGISTRY_TOKEN",
    );
    offline_clean(d.path());
    let mut envs = vec![("HOIST_CRATES_API", api.url())];
    if token {
        envs.push(("CARGO_REGISTRY_TOKEN", TOKEN.to_string()));
    }
    let r = run(d.path(), &envs, code);
    api.assert_only_get();
    if token {
        for req in api.requests() {
            assert_eq!(
                req.auth.as_deref(),
                Some(TOKEN),
                "crates.io wants the bare token"
            );
        }
    }
    r
}

#[test]
fn crates_ok_by_id_or_name() {
    for r in ["42", "publish"] {
        let r = crates_run(
            CRATES_LIST,
            200,
            &format!("provider_ref = \"{r}\""),
            true,
            0,
        );
        expect(&r, "ok", "token publish expires 2027-01-20");
        expect(&r, "ok", "last used 2026-11-20");
    }
}

#[test]
fn crates_missing_token_unknown() {
    let r = crates_run(CRATES_LIST, 200, "provider_ref = \"42\"", false, 0);
    expect(&r, "unknown", "CARGO_REGISTRY_TOKEN");
}

#[test]
fn crates_403_is_unknown_with_the_settings_link() {
    for status in [401, 403] {
        let r = crates_run("{}", status, "provider_ref = \"42\"", true, 0);
        expect(&r, "unknown", "https://crates.io/settings/tokens");
    }
}

#[test]
fn crates_not_listed_is_drift() {
    let r = crates_run(CRATES_LIST, 200, "provider_ref = \"7\"", true, 1);
    expect(&r, "drift", "not");
}

#[test]
fn crates_expired_is_drift() {
    let body = CRATES_LIST.replace("2027-01-20", "2026-11-01");
    let r = crates_run(&body, 200, "provider_ref = \"42\"", true, 1);
    expect(&r, "drift", "expired");
}

#[test]
fn crates_declared_scopes_outside_the_token_are_drift() {
    let r = crates_run(
        CRATES_LIST,
        200,
        "provider_ref = \"42\"\nscopes = [\"yank\"]",
        true,
        1,
    );
    expect(&r, "drift", "yank");
    let r = crates_run(
        CRATES_LIST,
        200,
        "provider_ref = \"42\"\nscopes = [\"publish-update\"]",
        true,
        0,
    );
    expect(&r, "ok", "token publish");
}

// ---------------------------------------------------------------- google

const SA: &str = "deployer@acme-proj.iam.gserviceaccount.com";
const KEYS_ROUTE: &str =
    "GET /v1/projects/acme-proj/serviceAccounts/deployer%40acme-proj.iam.gserviceaccount.com/keys";
const KEYS: &str = r#"{"keys":[{"name":"projects/acme-proj/serviceAccounts/deployer@acme-proj.iam.gserviceaccount.com/keys/k1","validAfterTime":"2026-11-01T00:00:00Z","validBeforeTime":"2027-01-20T00:00:00Z","keyType":"USER_MANAGED"}]}"#;

fn google_run(
    body: &str,
    status: u16,
    principal: &str,
    extra: &str,
    token: bool,
    code: i32,
) -> Value {
    let api = FakeApi::start(&[(KEYS_ROUTE, status, body, &[])]);
    let extra = format!("principal = \"{principal}\"\n{extra}");
    let toml = system("google", "GOOGLE_OAUTH_ACCESS_TOKEN", "deploy", &extra)
        .replace("kind = \"api_token\"", "kind = \"sa_key\"");
    let d = repo(&toml, "GOOGLE_OAUTH_ACCESS_TOKEN");
    offline_clean(d.path());
    let mut envs = vec![("HOIST_GOOGLE_API", api.url())];
    if token {
        envs.push(("GOOGLE_OAUTH_ACCESS_TOKEN", TOKEN.to_string()));
    }
    let r = run(d.path(), &envs, code);
    api.assert_only_get();
    r
}

#[test]
fn google_ok_single_user_key_without_ref() {
    let r = google_run(KEYS, 200, SA, "", true, 0);
    expect(&r, "ok", "key k1 valid until 2027-01-20");
}

#[test]
fn google_accepts_a_service_account_prefix_and_a_ref() {
    let r = google_run(
        KEYS,
        200,
        &format!("sa {SA}"),
        "provider_ref = \"k1\"",
        true,
        0,
    );
    expect(&r, "ok", "key k1");
}

#[test]
fn google_missing_token_unknown() {
    let r = google_run(KEYS, 200, SA, "", false, 0);
    expect(&r, "unknown", "GOOGLE_OAUTH_ACCESS_TOKEN");
}

#[test]
fn google_principal_without_at_is_unknown() {
    let r = google_run(KEYS, 200, "someone", "", true, 0);
    expect(&r, "unknown", "service account email");
}

#[test]
fn google_401_403_unknown() {
    for status in [401, 403] {
        let r = google_run("{}", status, SA, "", true, 0);
        expect(&r, "unknown", "GOOGLE_OAUTH_ACCESS_TOKEN");
    }
}

#[test]
fn google_disabled_key_is_drift() {
    let body = KEYS.replace("\"keyType\"", "\"disabled\":true,\"keyType\"");
    let r = google_run(&body, 200, SA, "", true, 1);
    expect(&r, "drift", "disabled");
}

#[test]
fn google_expiry_differs_and_never_is_9999() {
    let body = KEYS.replace("2027-01-20", "2027-02-02");
    let r = google_run(&body, 200, SA, "", true, 1);
    expect(&r, "drift", "2027-02-02");
    let body = KEYS.replace("2027-01-20T00:00:00Z", "9999-12-31T23:59:59Z");
    let r = google_run(&body, 200, SA, "", true, 1);
    expect(&r, "drift", "never expires");
}

#[test]
fn google_several_keys_need_a_ref_and_old_keys_are_drift() {
    let two = KEYS.replace(
        "]}",
        r#",{"name":"projects/p/serviceAccounts/x/keys/k2","validAfterTime":"2026-01-01T00:00:00Z","validBeforeTime":"2027-01-20T00:00:00Z","keyType":"USER_MANAGED"}]}"#,
    );
    let r = google_run(&two, 200, SA, "", true, 0);
    expect(&r, "unknown", "key id");
    let r = google_run(&two, 200, SA, "provider_ref = \"k1\"", true, 1);
    expect(&r, "drift", "1 other user-managed key");
}

#[test]
fn google_key_not_listed_is_drift() {
    let r = google_run(KEYS, 200, SA, "provider_ref = \"zz\"", true, 1);
    expect(&r, "drift", "not");
}

// ---------------------------------------------------------------- fly

struct Stub {
    dir: tempfile::TempDir,
}

impl Stub {
    /// A `fly` that needs FLY_API_TOKEN in env, refuses a canary in argv, logs argv and prints
    /// the given stdout for `tokens list` / `secrets list` (None: exits 1 like an old flyctl).
    fn new(tokens: Option<&str>, secrets: &str) -> Stub {
        let dir = tempfile::tempdir_in(common::tmpdir()).unwrap();
        fs::write(dir.path().join("tokens.out"), tokens.unwrap_or("")).unwrap();
        fs::write(dir.path().join("secrets.out"), secrets).unwrap();
        let tokens_cmd = if tokens.is_some() {
            "cat tokens.out".to_string()
        } else {
            "echo 'Error: unknown flag: --json' >&2; exit 1".to_string()
        };
        let body = format!(
            "#!/bin/sh\ncd '{d}'\necho \"$@\" >> argv.log\n\
             [ -n \"$FLY_API_TOKEN\" ] || exit 9\n\
             case \"$*\" in *canary*) exit 8;; esac\n\
             case \"$1 $2\" in\n\
             \"tokens list\") {tokens_cmd};;\n\
             \"secrets list\") cat secrets.out;;\n\
             *) exit 7;;\nesac\n",
            d = dir.path().display()
        );
        common::write_script(&dir.path().join("fly"), body.as_bytes(), 0o755);
        Stub { dir }
    }

    fn path(&self) -> String {
        self.dir.path().join("fly").display().to_string()
    }

    fn argv(&self) -> Vec<String> {
        fs::read_to_string(self.dir.path().join("argv.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

const FLY_TOKENS: &str =
    r#"[{"id":"tok_123","name":"deploy","expiresAt":"2027-01-20T00:00:00Z","revokedAt":null}]"#;

fn fly_system(extra: &str, storage: &str) -> String {
    let toml = system("fly", "FLY_API_TOKEN", "deploy", extra);
    toml.replace(
        "storage = [{ kind = \"gh-env-secret\", repo = \"acme/app\", env = \"site\", key = \"FLY_API_TOKEN\" }]",
        &format!(
            "storage = [{{ kind = \"gh-env-secret\", repo = \"acme/app\", env = \"site\", key = \"FLY_API_TOKEN\" }}{storage}]"
        ),
    )
}

fn fly_run(stub: &Stub, toml: &str, token: bool, code: i32) -> Value {
    let d = repo(toml, "FLY_API_TOKEN");
    offline_clean(d.path());
    let mut envs = vec![("HOIST_FLY", stub.path())];
    if token {
        envs.push(("FLY_API_TOKEN", TOKEN.to_string()));
    }
    let r = run(d.path(), &envs, code);
    for line in stub.argv() {
        assert!(
            line.starts_with("tokens list") || line.starts_with("secrets list"),
            "unexpected fly call: {line}"
        );
        assert!(!line.contains("canary"), "token in argv: {line}");
    }
    r
}

const FLY_EXTRA: &str = "provider_ref = \"tok_123\"\nresources = [\"app:myapp\"]";

#[test]
fn fly_ok_reads_only_the_token_list() {
    let stub = Stub::new(Some(FLY_TOKENS), "[]");
    let r = fly_run(&stub, &fly_system(FLY_EXTRA, ""), true, 0);
    expect(&r, "ok", "token deploy expires 2027-01-20");
    assert_eq!(stub.argv(), vec!["tokens list -a myapp --json"]);
}

#[test]
fn fly_app_from_a_consumer_runtime_and_keys_in_any_case() {
    let stub = Stub::new(
        Some(r#"{"tokens":[{"ID":"tok_123","Name":"deploy","ExpiresAt":"2027-01-20T00:00:00Z"}]}"#),
        "[]",
    );
    let toml = fly_system("provider_ref = \"deploy\"", "").replace(
        "environment = \"site\" }]",
        "environment = \"site\" }, { runtime = \"fly:myapp\" }]",
    );
    let r = fly_run(&stub, &toml, true, 0);
    expect(&r, "ok", "token deploy");
}

#[test]
fn fly_missing_token_unknown_and_nothing_runs() {
    let stub = Stub::new(Some(FLY_TOKENS), "[]");
    let r = fly_run(&stub, &fly_system(FLY_EXTRA, ""), false, 0);
    expect(&r, "unknown", "FLY_API_TOKEN");
    assert!(stub.argv().is_empty());
}

#[test]
fn fly_without_an_app_is_unknown() {
    let stub = Stub::new(Some(FLY_TOKENS), "[]");
    let r = fly_run(
        &stub,
        &fly_system("provider_ref = \"tok_123\"", ""),
        true,
        0,
    );
    expect(&r, "unknown", "app:<name>");
}

#[test]
fn fly_without_provider_ref_is_unknown() {
    let stub = Stub::new(Some(FLY_TOKENS), "[]");
    let r = fly_run(
        &stub,
        &fly_system("resources = [\"app:myapp\"]", ""),
        true,
        0,
    );
    expect(&r, "unknown", "Fly token id");
}

#[test]
fn fly_not_found_revoked_and_expiry_are_drift() {
    let stub = Stub::new(Some(FLY_TOKENS), "[]");
    let r = fly_run(
        &stub,
        &fly_system("provider_ref = \"other\"\nresources = [\"fly:myapp\"]", ""),
        true,
        1,
    );
    expect(&r, "drift", "not");
    let revoked = FLY_TOKENS.replace("null", "\"2026-11-20T00:00:00Z\"");
    let stub = Stub::new(Some(&revoked), "[]");
    let r = fly_run(&stub, &fly_system(FLY_EXTRA, ""), true, 1);
    expect(&r, "drift", "revoked");
    let moved = FLY_TOKENS.replace("2027-01-20", "2027-05-05");
    let stub = Stub::new(Some(&moved), "[]");
    let r = fly_run(&stub, &fly_system(FLY_EXTRA, ""), true, 1);
    expect(&r, "drift", "2027-05-05");
}

#[test]
fn fly_without_json_support_is_unknown() {
    let stub = Stub::new(None, "[]");
    let r = fly_run(&stub, &fly_system(FLY_EXTRA, ""), true, 0);
    expect(&r, "unknown", "no JSON");
    let stub = Stub::new(Some("ID NAME\ntok_123 deploy\n"), "[]");
    let r = fly_run(&stub, &fly_system(FLY_EXTRA, ""), true, 0);
    expect(&r, "unknown", "no JSON");
}

#[test]
fn fly_not_installed_is_unknown() {
    let d = repo(&fly_system(FLY_EXTRA, ""), "FLY_API_TOKEN");
    let r = run(d.path(), &[("FLY_API_TOKEN", TOKEN.to_string())], 0);
    expect(&r, "unknown", "fly CLI not found");
}

#[test]
fn fly_sweep_reports_missing_and_undeclared_secrets() {
    let stub = Stub::new(
        Some(FLY_TOKENS),
        r#"[{"Name":"STRAY","Digest":"x"},{"name":"PRESENT"}]"#,
    );
    let storage = ", { kind = \"fly-secret\", app = \"myapp\", key = \"PRESENT\" }, { kind = \"fly-secret\", app = \"myapp\", key = \"ABSENT\" }";
    let r = fly_run(&stub, &fly_system(FLY_EXTRA, storage), true, 1);
    let items = r["items"].as_array().unwrap();
    let find = |kind: &str, key: &str| {
        items
            .iter()
            .find(|i| i["kind"] == kind && i["key"] == key)
            .unwrap_or_else(|| panic!("no {kind} {key}: {r}"))
    };
    assert_eq!(find("undeclared", "STRAY")["action"], "drift");
    let storage: Vec<&Value> = items
        .iter()
        .filter(|i| i["kind"] == "live_storage")
        .collect();
    assert_eq!(storage.len(), 2, "{r}");
    assert!(storage.iter().any(|i| i["action"] == "ok"));
    assert!(
        storage
            .iter()
            .any(|i| i["action"] == "drift" && i.to_string().contains("missing fly:myapp/ABSENT"))
    );
    assert!(
        stub.argv()
            .iter()
            .all(|l| l.starts_with("tokens list") || l.starts_with("secrets list"))
    );
    assert!(
        stub.argv()
            .iter()
            .any(|l| l == "secrets list -a myapp --json")
    );
}

#[test]
fn fly_sweep_never_sends_another_providers_value_to_fly() {
    let dir = tempfile::tempdir_in(common::tmpdir()).unwrap();
    let body = format!(
        "#!/bin/sh\necho \"tok=$FLY_API_TOKEN\" >> '{d}/seen.log'\necho '[]'\n",
        d = dir.path().display()
    );
    common::write_script(&dir.path().join("fly"), body.as_bytes(), 0o755);
    let toml = system("cloudflare", "CLOUDFLARE_API_TOKEN", "deploy", "").replace(
        "storage = [{ kind = \"gh-env-secret\", repo = \"acme/app\", env = \"site\", key = \"CLOUDFLARE_API_TOKEN\" }]",
        "storage = [{ kind = \"fly-secret\", app = \"myapp\", key = \"CLOUDFLARE_API_TOKEN\" }]",
    );
    let d = repo(&toml, "CLOUDFLARE_API_TOKEN");
    let envs = [
        ("HOIST_FLY", dir.path().join("fly").display().to_string()),
        ("CLOUDFLARE_API_TOKEN", "canary-not-a-fly-token".to_string()),
        ("HOIST_CLOUDFLARE_API", "http://127.0.0.1:9".to_string()),
    ];
    let mut cmd = hoist();
    cmd.env("HOIST_NOW", NOW);
    for (k, v) in &envs {
        cmd.env(k, v);
    }
    let out = cmd
        .arg("-C")
        .arg(d.path())
        .args(["system", "check", "--live", "--json"])
        .output()
        .unwrap();
    let seen = fs::read_to_string(dir.path().join("seen.log")).unwrap_or_default();
    assert!(!seen.contains("canary"), "value reached fly: {seen}");
    assert!(seen.is_empty(), "fly ran without a Fly token: {seen}");
    let r: Value = serde_json::from_slice(&out.stdout).unwrap();
    let item = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "live" && i["key"] == "fly:myapp")
        .unwrap_or_else(|| panic!("no live fly:myapp item: {r}"));
    assert_eq!(item["action"], "unknown");
    assert!(item.to_string().contains("FLY_API_TOKEN"), "{item}");
}

#[test]
fn fly_sweep_failed_secrets_list_is_unknown_with_a_fix() {
    let stub = Stub::new(Some(FLY_TOKENS), "[]");
    fs::write(stub.dir.path().join("secrets.out"), "not json").unwrap();
    let storage = ", { kind = \"fly-secret\", app = \"myapp\", key = \"PRESENT\" }";
    let d = repo(&fly_system(FLY_EXTRA, storage), "FLY_API_TOKEN");
    let r = run(
        d.path(),
        &[
            ("HOIST_FLY", stub.path()),
            ("FLY_API_TOKEN", TOKEN.to_string()),
        ],
        0,
    );
    let item = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "live" && i["key"] == "fly:myapp")
        .unwrap_or_else(|| panic!("no live fly:myapp item: {r}"));
    assert_eq!(item["action"], "unknown");
}
