//! `hoist secrets rotate <credential-id>` from system.toml and `hoist system stamp` (spec §17).

mod common;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{FakeApi, RouteSpec, hoist, stub};
use crypto_box::SecretKey;
use crypto_box::aead::OsRng;
use serde_json::Value;
use std::fs;
use std::path::Path;

const NOW: &str = "2026-11-27T12:00:00Z";
const CF_CANARY: &str = "hoist-canary-sys-rotate-cf";
const FLY_CANARY: &str = "FlyV1 canary-fly-new-token";

const HEAD: &str = r#"version = 1

[system]
name = "acme/x"
owner = "alice"

[[provider]]
id = "cloudflare"
capability = "dns+cdn"

[[provider]]
id = "fly"
capability = "paas.container"

[[provider]]
id = "npm"
capability = "registry.npm"
console = "https://npm.example/tokens"

[[provider]]
id = "github"
capability = "vcs+ci"

"#;

const CF_CRED: &str = r#"# the site token, rotated by hoist
[[credential]]
id = "site-deploy"
provider = "cloudflare"
kind = "api_token"
role = "deploy"
purpose = "Deploy the site"
owner = "alice"
created_at = "2026-08-01"   # old
created_by = "alice"
expires_at = "2026-12-01"
max_ttl = "90d"
provider_ref = "tok-1"
rotation = { strategy = "auto" }
storage = [
  { kind = "gh-secret", repo = "acme/x", key = "CLOUDFLARE_API_TOKEN" },
  { kind = "gh-env-secret", repo = "acme/x", env = "site", key = "CF_TOKEN" },
]

"#;

const NPM_CRED: &str = r#"[[credential]]
id = "npm-publish"
provider = "npm"
kind = "api_token"
role = "publish"
purpose = "Publish"
owner = "alice"
expires_at = "2027-01-01"
rotation = { strategy = "assisted", steps = ["open the token page", "create a granular token"] }
storage = [
  { kind = "gh-secret", repo = "acme/x", key = "NPM_TOKEN" },
  { kind = "sops", path = "env/prod.sops.yaml", key = "NPM_TOKEN" },
]
"#;

fn system(parts: &[&str]) -> String {
    let mut s = HEAD.to_string();
    for p in parts {
        s.push_str(p);
    }
    s
}

fn repo(text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir_in(common::tmpdir()).unwrap();
    fs::write(dir.path().join("system.toml"), text).unwrap();
    dir
}

fn keypair() -> (SecretKey, String) {
    let sk = SecretKey::generate(&mut OsRng);
    let b64 = STANDARD.encode(sk.public_key().as_bytes());
    (sk, format!(r#"{{"key_id":"kid-1","key":"{b64}"}}"#))
}

const TOKENS: &str = r#"{"success":true,"errors":[],"messages":[],"result":[
 {"id":"tok-0","name":"unrelated","status":"active","policies":[]},
 {"id":"tok-1","name":"hoist-site","status":"active","policies":[
  {"id":"p1","effect":"allow","resources":{"com.cloudflare.api.account.acc":"*"},
   "permission_groups":[{"id":"g-pages","name":"Pages Write"}]}]}]}"#;
const DELETED: &str = r#"{"success":true,"errors":[],"messages":[],"result":{"id":"x"}}"#;
const PUT_REPO: &str = "PUT /repos/acme/x/actions/secrets/CLOUDFLARE_API_TOKEN";
const PUT_ENV: &str = "PUT /repos/acme/x/environments/site/secrets/CF_TOKEN";
const VERIFY: &str = "GET /user/tokens/verify";
const DEL_OLD: &str = "DELETE /user/tokens/tok-1";
const DEL_NEW: &str = "DELETE /user/tokens/new-id";

fn minted() -> String {
    format!(
        r#"{{"success":true,"errors":[],"messages":[],"result":{{"id":"new-id","name":"hoist-site","value":"{CF_CANARY}"}}}}"#
    )
}

fn cf_api(key_json: &str, over: &[(&str, u16, &str)]) -> FakeApi {
    let minted = minted();
    let base: Vec<(&str, u16, &str)> = vec![
        ("GET /user/tokens", 200, TOKENS),
        ("POST /user/tokens", 200, &minted),
        (VERIFY, 200, "cloudflare/token_verify_active.json"),
        (
            "GET /repos/acme/x/actions/secrets/public-key",
            200,
            key_json,
        ),
        (
            "GET /repos/acme/x/environments/site/secrets/public-key",
            200,
            key_json,
        ),
        (PUT_REPO, 201, "empty.json"),
        (PUT_ENV, 201, "empty.json"),
        (DEL_OLD, 200, DELETED),
        (DEL_NEW, 200, DELETED),
    ];
    let routes: Vec<RouteSpec> = base
        .into_iter()
        .filter(|(k, ..)| !over.iter().any(|(o, ..)| o == k))
        .chain(over.iter().copied())
        .map(|(k, s, b)| (k, s, b, &[][..]))
        .collect();
    FakeApi::start(&routes)
}

struct Run {
    json: Value,
    all: String,
}

fn rotate(api: &FakeApi, dir: &Path, id: &str, code: i32) -> Run {
    let out = hoist()
        .env("HOIST_NOW", NOW)
        .env("HOIST_GITHUB_API", api.url())
        .env("HOIST_CLOUDFLARE_API", api.url())
        .env("HOIST_FLY_API", api.url())
        .env("HOIST_MAX_SLEEP_MS", "0")
        .env("GITHUB_TOKEN", "gh-test-token")
        .env("CLOUDFLARE_API_TOKEN", "cf-test-token")
        .env("CLOUDFLARE_ACCOUNT_ID", "acc")
        .arg("-C")
        .arg(dir)
        .args(["secrets", "rotate", id, "--json"])
        .assert()
        .code(code)
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let all = format!("{stdout}{}", String::from_utf8(out.stderr).unwrap());
    for canary in [CF_CANARY, FLY_CANARY, "test-token"] {
        assert!(!all.contains(canary), "{canary} leaked: {all}");
    }
    Run {
        json: serde_json::from_str(&stdout).unwrap_or(Value::Null),
        all,
    }
}

fn writes(api: &FakeApi) -> Vec<String> {
    api.requests()
        .iter()
        .filter(|r| r.method != "GET")
        .map(|r| format!("{} {}", r.method, r.path))
        .collect()
}

fn unseal(sk: &SecretKey, body: &str) -> String {
    let body: Value = serde_json::from_str(body).unwrap();
    let bytes = STANDARD
        .decode(body["encrypted_value"].as_str().unwrap())
        .unwrap();
    String::from_utf8(sk.unseal(&bytes).unwrap()).unwrap()
}

fn read(dir: &Path) -> String {
    fs::read_to_string(dir.join("system.toml")).unwrap()
}

#[test]
fn cloudflare_rotation_is_dual_phase_and_stamps_system_toml() {
    let text = system(&[CF_CRED, NPM_CRED]);
    let dir = repo(&text);
    let (sk, key_json) = keypair();
    let api = cf_api(&key_json, &[]);
    let run = rotate(&api, dir.path(), "site-deploy", 0);
    assert_eq!(run.json["command"], "secrets rotate");
    assert_eq!(run.json["exit_code"], 0);
    let reqs = api.requests();
    let pos = |m: &str, p: &str| {
        reqs.iter()
            .position(|r| r.method == m && r.path == p)
            .unwrap_or_else(|| panic!("no {m} {p}"))
    };
    let post = pos("POST", "/user/tokens");
    let verify_new = reqs
        .iter()
        .position(|r| {
            r.path == "/user/tokens/verify"
                && r.auth.as_deref() == Some(&format!("Bearer {CF_CANARY}"))
        })
        .expect("new token verified");
    let put1 = pos("PUT", "/repos/acme/x/actions/secrets/CLOUDFLARE_API_TOKEN");
    let put2 = pos("PUT", "/repos/acme/x/environments/site/secrets/CF_TOKEN");
    let del = pos("DELETE", "/user/tokens/tok-1");
    assert!(post < verify_new && verify_new < put1.min(put2), "{reqs:?}");
    assert!(put1.max(put2) < del, "old revoked last");
    assert_eq!(writes(&api).len(), 4, "{:?}", writes(&api));
    // The new value is sealed into both storages; the create body copies the old policies.
    for i in [put1, put2] {
        assert_eq!(unseal(&sk, reqs[i].body.as_ref().unwrap()), CF_CANARY);
    }
    let body: Value = serde_json::from_str(reqs[post].body.as_ref().unwrap()).unwrap();
    assert_eq!(body["name"], "hoist-site");
    assert_eq!(body["expires_on"], "2027-02-25T00:00:00Z");
    assert_eq!(body["policies"][0]["effect"], "allow");
    assert_eq!(
        body["policies"][0]["resources"]["com.cloudflare.api.account.acc"],
        "*"
    );
    assert_eq!(body["policies"][0]["permission_groups"][0]["id"], "g-pages");
    // The roll endpoint would kill the old value at once.
    assert!(!reqs.iter().any(|r| r.path.ends_with("/value")));
    // system.toml: stamped, comment kept, the other credential untouched.
    let after = read(dir.path());
    assert!(after.contains("# the site token, rotated by hoist"));
    assert!(after.contains(NPM_CRED), "other credential byte-identical");
    assert!(after.contains("created_at = \"2026-11-27\""));
    assert!(after.contains("expires_at = \"2027-02-25\""));
    assert!(after.contains("created_via = \"hoist secrets rotate site-deploy\""));
    assert!(after.contains("created_by = \"alice\""));
    assert!(after.contains("provider_ref = \"token id new-id\""));
    assert!(!after.contains(CF_CANARY));
    let items = run.json["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|i| i["kind"] == "stamp" && i["key"] == "site-deploy")
    );
    assert!(
        items.iter().any(|i| i["reason"] == "revoked tok-1"),
        "{items:?}"
    );
}

#[test]
fn failed_verification_revokes_the_new_token_and_touches_nothing_else() {
    let text = system(&[CF_CRED]);
    let dir = repo(&text);
    let (_, key_json) = keypair();
    let api = cf_api(
        &key_json,
        &[(VERIFY, 200, "cloudflare/token_verify_inactive.json")],
    );
    let run = rotate(&api, dir.path(), "site-deploy", 1);
    let w = writes(&api);
    assert!(w.contains(&DEL_NEW.to_string()), "{w:?}");
    assert!(!w.contains(&DEL_OLD.to_string()), "{w:?}");
    assert!(!w.iter().any(|x| x.starts_with("PUT")), "{w:?}");
    assert_eq!(read(dir.path()), text);
    assert!(
        run.json["items"][0]["reason"]
            .as_str()
            .unwrap()
            .starts_with("failed: ")
    );
}

#[test]
fn partial_write_keeps_both_tokens_and_names_the_holders() {
    let text = system(&[CF_CRED]);
    let dir = repo(&text);
    let (_, key_json) = keypair();
    let api = cf_api(&key_json, &[(PUT_ENV, 500, "empty.json")]);
    let run = rotate(&api, dir.path(), "site-deploy", 1);
    let w = writes(&api);
    assert!(w.contains(&PUT_REPO.to_string()), "{w:?}");
    assert!(!w.iter().any(|x| x.starts_with("DELETE")), "{w:?}");
    assert!(run.all.contains("gh:CLOUDFLARE_API_TOKEN"), "{}", run.all);
    assert_eq!(read(dir.path()), text);
}

#[test]
fn assisted_and_manual_make_no_requests_and_list_human_steps() {
    let manual = NPM_CRED
        .replace("assisted", "manual")
        .replace("npm-publish", "npm-manual");
    let text = system(&[NPM_CRED, "\n", &manual]);
    let dir = repo(&text);
    let api = cf_api("{}", &[]);
    let run = rotate(&api, dir.path(), "npm-publish", 2);
    assert!(api.requests().is_empty());
    assert_eq!(read(dir.path()), text);
    let items = run.json["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["area"], "system");
    assert_eq!(items[0]["kind"], "rotation");
    assert_eq!(items[0]["key"], "npm-publish");
    assert_eq!(items[0]["action"], "pending");
    assert_eq!(items[0]["reason"], "assisted: human steps");
    let steps = run.json["human_steps"].as_array().unwrap();
    let ids: Vec<&str> = steps.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids[0], "rotate:npm-publish:1");
    assert_eq!(ids[1], "rotate:npm-publish:2");
    assert_eq!(*ids.last().unwrap(), "stamp:npm-publish");
    assert_eq!(steps.len(), 5, "2 steps + 2 storages + stamp: {steps:?}");
    assert_eq!(steps[0]["url"], "https://npm.example/tokens");
    assert!(
        steps[2]["title"]
            .as_str()
            .unwrap()
            .contains("hoist secrets set NPM_TOKEN")
    );
    assert!(
        steps[3]["title"]
            .as_str()
            .unwrap()
            .contains("hoist secrets edit env/prod.sops.yaml")
    );
    assert!(
        steps[4]["title"]
            .as_str()
            .unwrap()
            .contains("hoist system stamp npm-publish --created YYYY-MM-DD --expires YYYY-MM-DD")
    );
    run_manual(&api, dir.path());
}

fn run_manual(api: &FakeApi, dir: &Path) {
    let run = rotate(api, dir, "npm-manual", 2);
    assert_eq!(run.json["items"][0]["reason"], "manual: human steps");
    assert!(api.requests().is_empty());
}

#[test]
fn preflight_errors_exit_3_before_any_write() {
    let (_, key_json) = keypair();
    let sops = CF_CRED.replace(
        "{ kind = \"gh-env-secret\", repo = \"acme/x\", env = \"site\", key = \"CF_TOKEN\" },",
        "{ kind = \"sops\", path = \"env/p.sops.yaml\", key = \"CF_TOKEN\" },",
    );
    let npm_auto = NPM_CRED.replace("assisted", "auto");
    let no_ref = CF_CRED.replace("provider_ref = \"tok-1\"\n", "");
    let unknown_ref = CF_CRED.replace("tok-1\"", "tok-missing\"");
    let revoked = CF_CRED.replace("provider_ref", "status = \"revoked\"\nprovider_ref");
    for (text, needle) in [
        (
            system(&[&sops]),
            "cannot be written by hoist; set rotation.strategy = \"assisted\"",
        ),
        (
            system(&[&npm_auto]),
            "auto rotation supports cloudflare and fly; use assisted",
        ),
        (system(&[&no_ref]), "provider_ref"),
        (system(&[&unknown_ref]), "tok-missing"),
        (system(&[&revoked]), "not active"),
    ] {
        let dir = repo(&text);
        let api = cf_api(&key_json, &[]);
        let id = if text.contains("npm-publish") {
            "npm-publish"
        } else {
            "site-deploy"
        };
        let run = rotate(&api, dir.path(), id, 3);
        let _ = run;
        assert!(writes(&api).is_empty(), "{needle}: {:?}", writes(&api));
        assert_eq!(read(dir.path()), text);
        let out = hoist()
            .env("HOIST_NOW", NOW)
            .env("HOIST_GITHUB_API", api.url())
            .env("HOIST_CLOUDFLARE_API", api.url())
            .env("GITHUB_TOKEN", "gh-test-token")
            .env("CLOUDFLARE_API_TOKEN", "cf-test-token")
            .arg("-C")
            .arg(dir.path())
            .args(["secrets", "rotate", id])
            .assert()
            .code(3)
            .get_output()
            .stderr
            .clone();
        let err = String::from_utf8(out).unwrap();
        assert!(err.contains(needle), "{needle} not in {err}");
    }
}

#[test]
fn missing_cloudflare_token_exits_3_without_requests() {
    let dir = repo(&system(&[CF_CRED]));
    let api = cf_api("{}", &[]);
    hoist()
        .env("GITHUB_TOKEN", "gh-test-token")
        .env("HOIST_GITHUB_API", api.url())
        .env("HOIST_CLOUDFLARE_API", api.url())
        .arg("-C")
        .arg(dir.path())
        .args(["secrets", "rotate", "site-deploy"])
        .assert()
        .code(3);
    assert!(api.requests().is_empty());
}

#[test]
fn legacy_unknown_name_message_mentions_credential_ids() {
    let dir = repo(&system(&[CF_CRED]));
    let out = hoist()
        .arg("-C")
        .arg(dir.path())
        .args(["secrets", "rotate", "NOPE"])
        .assert()
        .code(3)
        .get_output()
        .stderr
        .clone();
    let err = String::from_utf8(out).unwrap();
    assert!(
        err.contains("only minted secrets can be rotated: CLOUDFLARE_API_TOKEN"),
        "{err}"
    );
    assert!(err.contains("(or a system.toml credential id)"), "{err}");
}

#[test]
fn fly_rotation_creates_imports_over_stdin_and_revokes_old() {
    let fly_cred = r#"[[credential]]
id = "fly-deploy"
provider = "fly"
kind = "deploy_token"
role = "deploy"
purpose = "Deploy"
owner = "alice"
expires_at = "2026-12-20"
rotate_every = "30d"
provider_ref = "old-id"
rotation = { strategy = "auto" }
storage = [
  { kind = "fly-secret", app = "myapp", key = "FLY_KEY" },
  { kind = "gh-secret", repo = "acme/x", key = "FLY_API_TOKEN" },
]
"#;
    let text = system(&[fly_cred]);
    let dir = repo(&text);
    let tools = tempfile::tempdir_in(common::tmpdir()).unwrap();
    let t = tools.path().display();
    let script = format!(
        r#"case "$1 $2" in
"tokens list") if [ -f '{t}/listed' ]; then echo '[{{"id":"old-id"}},{{"id":"new-fly-id"}}]'; else touch '{t}/listed'; echo '[{{"id":"old-id"}}]'; fi;;
"tokens create") echo "{FLY_CANARY}";;
"secrets import") cat > '{t}/import.in';;
esac"#
    );
    let fly = stub(tools.path(), "fly", &script);
    let (sk, key_json) = keypair();
    let key_route = key_json.as_str();
    let routes: Vec<RouteSpec> = vec![
        ("GET /apps/myapp", 200, r#"{"name":"myapp"}"#, &[]),
        (
            "GET /repos/acme/x/actions/secrets/public-key",
            200,
            key_route,
            &[],
        ),
        (
            "PUT /repos/acme/x/actions/secrets/FLY_API_TOKEN",
            201,
            "empty.json",
            &[],
        ),
    ];
    let api = FakeApi::start(&routes);
    let out = hoist()
        .env("HOIST_NOW", NOW)
        .env("HOIST_FLY", &fly)
        .env("HOIST_FLY_API", api.url())
        .env("HOIST_GITHUB_API", api.url())
        .env("HOIST_MAX_SLEEP_MS", "0")
        .env("GITHUB_TOKEN", "gh-test-token")
        .env("FLY_API_TOKEN", "fly-test-token")
        .arg("-C")
        .arg(dir.path())
        .args(["secrets", "rotate", "fly-deploy", "--json"])
        .assert()
        .code(0)
        .get_output()
        .clone();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.contains("canary-fly-new-token"), "{all}");
    let log = fs::read_to_string(tools.path().join("fly.log")).unwrap();
    assert!(!log.contains("canary-fly-new-token"), "argv leaked: {log}");
    let cmds: Vec<&str> = log
        .lines()
        .map(|l| l.split(" env:").next().unwrap())
        .collect();
    assert_eq!(
        cmds,
        [
            "tokens list -a myapp --json",
            "tokens create deploy -a myapp --expiry 720h",
            "tokens list -a myapp --json",
            "secrets import -a myapp",
            "tokens revoke old-id",
        ]
    );
    let imported = fs::read_to_string(tools.path().join("import.in")).unwrap();
    assert_eq!(imported, format!("FLY_KEY={FLY_CANARY}\n"));
    let reqs = api.requests();
    let probe = reqs
        .iter()
        .find(|r| r.path == "/apps/myapp")
        .expect("new token tested");
    assert!(
        probe
            .auth
            .as_deref()
            .unwrap()
            .contains("canary-fly-new-token")
    );
    let put = reqs.iter().find(|r| r.method == "PUT").unwrap();
    assert_eq!(unseal(&sk, put.body.as_ref().unwrap()), FLY_CANARY);
    let after = read(dir.path());
    assert!(
        after.contains("provider_ref = \"token id new-fly-id\""),
        "{after}"
    );
    assert!(after.contains("created_at = \"2026-11-27\""));
    assert!(after.contains("expires_at = \"2026-12-27\""));
}

fn bare(api: &FakeApi, dir: &Path, args: &[&str], verbose: bool) -> std::process::Output {
    let mut cmd = hoist();
    cmd.env("HOIST_NOW", NOW)
        .env("HOIST_GITHUB_API", api.url())
        .env("HOIST_CLOUDFLARE_API", api.url())
        .env("HOIST_MAX_SLEEP_MS", "0")
        .env("GITHUB_TOKEN", "gh-test-token")
        .env("CLOUDFLARE_API_TOKEN", "cf-test-token")
        .env("CLOUDFLARE_ACCOUNT_ID", "acc")
        .arg("-C")
        .arg(dir)
        .args(args);
    if verbose {
        cmd.arg("-v");
    }
    cmd.output().unwrap()
}

#[test]
fn create_body_copies_the_old_condition_and_not_before() {
    let tokens = TOKENS.replace(
        r#""name":"hoist-site","status":"active","#,
        r#""name":"hoist-site","status":"active","not_before":"2026-01-01T00:00:00Z","condition":{"request.ip":{"in":["10.0.0.0/8"],"not_in":["10.1.0.0/16"]}},"#,
    );
    assert_ne!(tokens, TOKENS);
    let dir = repo(&system(&[CF_CRED]));
    let (_, key_json) = keypair();
    let api = cf_api(&key_json, &[("GET /user/tokens", 200, &tokens)]);
    rotate(&api, dir.path(), "site-deploy", 0);
    let reqs = api.requests();
    let post = reqs
        .iter()
        .find(|r| r.method == "POST" && r.path == "/user/tokens")
        .unwrap();
    let body: Value = serde_json::from_str(post.body.as_ref().unwrap()).unwrap();
    assert_eq!(body["condition"]["request.ip"]["in"][0], "10.0.0.0/8");
    assert_eq!(body["condition"]["request.ip"]["not_in"][0], "10.1.0.0/16");
    assert_eq!(body["not_before"], "2026-01-01T00:00:00Z");
}

#[test]
fn first_storage_write_failing_revokes_the_new_token_and_changes_nothing() {
    let text = system(&[CF_CRED]);
    let dir = repo(&text);
    let (_, key_json) = keypair();
    let api = cf_api(&key_json, &[(PUT_REPO, 500, "empty.json")]);
    let run = rotate(&api, dir.path(), "site-deploy", 1);
    let w = writes(&api);
    assert!(w.contains(&DEL_NEW.to_string()), "{w:?}");
    assert!(!w.contains(&DEL_OLD.to_string()), "{w:?}");
    assert!(!w.contains(&PUT_ENV.to_string()), "{w:?}");
    assert_eq!(read(dir.path()), text);
    assert!(run.all.contains("nothing was written"), "{}", run.all);
}

#[test]
fn failing_to_revoke_the_old_token_is_a_human_step_and_still_stamps() {
    let dir = repo(&system(&[CF_CRED]));
    let (_, key_json) = keypair();
    let api = cf_api(&key_json, &[(DEL_OLD, 500, "empty.json")]);
    let run = rotate(&api, dir.path(), "site-deploy", 1);
    let steps = run.json["human_steps"].as_array().unwrap();
    assert!(
        steps.iter().any(|s| s["id"] == "revoke-token:tok-1"),
        "{steps:?}"
    );
    let after = read(dir.path());
    assert!(after.contains("created_at = \"2026-11-27\""), "{after}");
    assert!(after.contains("provider_ref = \"token id new-id\""));
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 1, "no temp file left behind: {names:?}");
}

#[test]
fn verbose_rotation_never_prints_a_token_value() {
    let dir = repo(&system(&[CF_CRED]));
    let (_, key_json) = keypair();
    let api = cf_api(&key_json, &[]);
    let out = bare(
        &api,
        dir.path(),
        &["secrets", "rotate", "site-deploy", "--json"],
        true,
    );
    assert_eq!(out.status.code(), Some(0));
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!all.is_empty());
    for canary in [CF_CANARY, "test-token", "encrypted_value"] {
        assert!(!all.contains(canary), "{canary} leaked: {all}");
    }
}

#[test]
fn legacy_rotation_still_runs_with_a_valid_system_toml() {
    let dir = repo(&system(&[CF_CRED]));
    let api = cf_api("{}", &[]);
    let out = bare(
        &api,
        dir.path(),
        &["secrets", "rotate", "CLOUDFLARE_API_TOKEN"],
        false,
    );
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    // No hoist.toml here: the legacy path reports its own config error, not a system rotation.
    assert_eq!(out.status.code(), Some(3), "{err}");
    assert!(!err.contains("only minted secrets"), "{err}");
    assert!(writes(&api).is_empty());
    assert!(!api.requests().iter().any(|r| r.path == "/user/tokens"));
}

#[test]
fn invalid_system_toml_breaks_other_names_but_not_the_legacy_secret() {
    let dir = repo("version = 1\n[[credential\n");
    let api = cf_api("{}", &[]);
    let legacy = bare(
        &api,
        dir.path(),
        &["secrets", "rotate", "CLOUDFLARE_API_TOKEN"],
        false,
    );
    let err = String::from_utf8_lossy(&legacy.stderr).to_string();
    assert_eq!(legacy.status.code(), Some(3), "{err}");
    assert!(
        !err.contains("system.toml"),
        "legacy path ignores it: {err}"
    );
    let other = bare(
        &api,
        dir.path(),
        &["secrets", "rotate", "site-deploy"],
        false,
    );
    let err = String::from_utf8_lossy(&other.stderr).to_string();
    assert_eq!(other.status.code(), Some(3), "{err}");
    assert!(err.contains("system.toml"), "{err}");
    assert!(!err.contains("only minted secrets"), "{err}");
}

const FLY_CRED: &str = r#"[[credential]]
id = "fly-deploy"
provider = "fly"
kind = "deploy_token"
role = "deploy"
purpose = "Deploy"
owner = "alice"
expires_at = "2026-12-20"
rotate_every = "30d"
provider_ref = "REF"
rotation = { strategy = "auto" }
storage = [
  { kind = "fly-secret", app = "myapp", key = "FLY_KEY" },
]
"#;

fn fly_rotate(list: &str, reference: &str, code: i32) -> (String, String, String) {
    let text = system(&[&FLY_CRED.replace("REF", reference)]);
    let dir = repo(&text);
    let tools = tempfile::tempdir_in(common::tmpdir()).unwrap();
    let t = tools.path().display();
    let script = format!(
        r#"case "$1 $2" in
"tokens list") if [ -f '{t}/listed' ]; then echo '[{{"id":"old-id","name":"deploy"}},{{"id":"new-fly-id"}}]'; else touch '{t}/listed'; echo '{list}'; fi;;
"tokens create") echo "{FLY_CANARY}";;
"secrets import") cat > '{t}/import.in';;
esac"#
    );
    let fly = stub(tools.path(), "fly", &script);
    let api = FakeApi::start(&[("GET /apps/myapp", 200, r#"{"name":"myapp"}"#, &[])]);
    hoist()
        .env("HOIST_NOW", NOW)
        .env("HOIST_FLY", &fly)
        .env("HOIST_FLY_API", api.url())
        .env("HOIST_MAX_SLEEP_MS", "0")
        .env("FLY_API_TOKEN", "fly-test-token")
        .arg("-C")
        .arg(dir.path())
        .args(["secrets", "rotate", "fly-deploy"])
        .assert()
        .code(code);
    let log = fs::read_to_string(tools.path().join("fly.log")).unwrap_or_default();
    (log, read(dir.path()), text)
}

#[test]
fn fly_name_ref_resolves_to_the_token_id_before_any_write() {
    let (log, _, _) = fly_rotate(r#"[{"id":"old-id","name":"deploy"}]"#, "deploy", 0);
    assert!(log.contains("tokens revoke old-id"), "{log}");
}

#[test]
fn fly_unresolvable_ref_exits_3_before_any_write() {
    let (log, after, text) = fly_rotate(r#"[{"id":"old-id","name":"deploy"}]"#, "nope", 3);
    assert!(
        !log.contains("create") && !log.contains("import") && !log.contains("revoke"),
        "{log}"
    );
    assert_eq!(after, text);
}

// ---- `hoist system stamp` ----

fn stamp(dir: &Path, args: &[&str], code: i32) -> Value {
    let out = hoist()
        .arg("-C")
        .arg(dir)
        .args(["system", "stamp"])
        .args(args)
        .arg("--json")
        .assert()
        .code(code)
        .get_output()
        .clone();
    serde_json::from_slice(&out.stdout).unwrap_or(Value::Null)
}

#[test]
fn stamp_edits_only_that_credential_and_is_idempotent() {
    let text = system(&[CF_CRED, NPM_CRED]);
    let dir = repo(&text);
    let args = [
        "npm-publish",
        "--created",
        "2026-11-27",
        "--expires",
        "2027-02-25",
        "--by",
        "bob",
        "--via",
        "npm website",
        "--ref",
        "tok-9",
    ];
    let r = stamp(dir.path(), &args, 0);
    assert_eq!(r["command"], "system stamp");
    assert_eq!(r["items"][0]["area"], "system");
    assert_eq!(r["items"][0]["kind"], "stamp");
    assert_eq!(r["items"][0]["key"], "npm-publish");
    assert_eq!(r["items"][0]["action"], "update");
    let after = read(dir.path());
    assert!(after.contains("# the site token, rotated by hoist"));
    assert!(after.contains(CF_CRED), "other credential byte-identical");
    assert!(after.contains("created_at = \"2026-11-27\""));
    assert!(after.contains("expires_at = \"2027-02-25\""));
    assert!(after.contains("created_by = \"bob\""));
    assert!(after.contains("created_via = \"npm website\""));
    assert!(after.contains("provider_ref = \"tok-9\""));
    assert!(!after.contains("expires_at = \"2027-01-01\""));
    let again = stamp(dir.path(), &args, 0);
    assert_eq!(again["items"][0]["action"], "ok");
    assert_eq!(read(dir.path()), after);
    // `never` and a custom --file.
    stamp(
        dir.path(),
        &[
            "npm-publish",
            "--created",
            "2026-11-27",
            "--expires",
            "never",
        ],
        0,
    );
    assert!(read(dir.path()).contains("expires_at = \"never\""));
}

#[test]
fn stamp_rejects_unknown_ids_and_bad_dates_without_writing() {
    let text = system(&[CF_CRED]);
    let dir = repo(&text);
    stamp(dir.path(), &["nope", "--created", "2026-11-27"], 3);
    stamp(dir.path(), &["site-deploy", "--created", "2026-13-01"], 3);
    stamp(
        dir.path(),
        &[
            "site-deploy",
            "--created",
            "2026-11-27",
            "--expires",
            "soon",
        ],
        3,
    );
    assert_eq!(read(dir.path()), text);
}
