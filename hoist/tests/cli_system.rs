mod common;

use serde_json::Value;
use std::fs;
use std::path::Path;

const NOW: &str = "2026-11-27T12:00:00Z";

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
          NPM_TOKEN: ${{ secrets.NPM_TOKEN }}
          TOKEN: ${{ secrets.GITHUB_TOKEN }}
"#;

const GOOD: &str = r#"version = 1

[system]
name = "acme/app"
owner = "alice"
description = "Test system"

[[provider]]
id = "cloudflare"
capability = "dns+cdn"
account = "acme"
clis = [{ tool = "wrangler", version = "4.40.0", mise = "npm:wrangler" }]
alternatives = ["bunny"]

[[provider]]
id = "npm"
capability = "registry.npm"

[[credential]]
id = "site-deploy"
provider = "cloudflare"
kind = "api_token"
role = "deploy"
purpose = "Deploy the site"
owner = "alice"
created_at = "2026-11-01"
expires_at = "2027-01-20"
max_ttl = "90d"
rotation = { strategy = "auto", command = "hoist secrets rotate CLOUDFLARE_API_TOKEN" }
storage = [{ kind = "gh-env-secret", repo = "acme/app", env = "site", key = "CLOUDFLARE_API_TOKEN" }]
consumers = [{ workflow = "deploy.yml", job = "deploy", environment = "site" }]

[[credential]]
id = "npm-publish"
provider = "npm"
kind = "api_token"
role = "publish"
purpose = "Publish packages"
owner = "alice"
created_at = "2026-11-01"
expires_at = "2027-01-01"
keyless_exempt = "first publish needs a token"
storage = [{ kind = "gh-env-secret", repo = "acme/app", env = "site", key = "NPM_TOKEN" }]
consumers = [{ workflow = "deploy.yml", job = "deploy", environment = "site" }]
"#;

fn repo(system: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir_in(common::tmpdir()).unwrap();
    let wf = dir.path().join(".github/workflows");
    fs::create_dir_all(&wf).unwrap();
    fs::write(wf.join("deploy.yml"), WORKFLOW).unwrap();
    if let Some(s) = system {
        fs::write(dir.path().join("system.toml"), s).unwrap();
    }
    dir
}

fn run(dir: &Path, args: &[&str], code: i32) -> Value {
    let out = common::hoist()
        .env("HOIST_NOW", NOW)
        .arg("-C")
        .arg(dir)
        .args(args)
        .arg("--json")
        .assert()
        .code(code)
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).expect("one JSON object")
}

fn items<'a>(r: &'a Value, kind: &str) -> Vec<&'a Value> {
    r["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == kind)
        .collect()
}

#[test]
fn check_clean_system_exits_0() {
    let d = repo(Some(GOOD));
    let r = run(d.path(), &["system", "check"], 0);
    assert_eq!(r["command"], "system check");
    assert_eq!(r["version"], 1);
    assert_eq!(r["ok"], true);
    assert_eq!(items(&r, "credential").len(), 2, "{r}");
    assert!(
        r["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["area"] == "system")
    );
}

#[test]
fn unknown_key_exits_3_naming_it() {
    let d = repo(Some(&GOOD.replace(
        "owner = \"alice\"\ndescription",
        "ownr = \"x\"\nowner = \"alice\"\ndescription",
    )));
    let out = common::hoist()
        .arg("-C")
        .arg(d.path())
        .args(["system", "check"])
        .assert()
        .code(3)
        .get_output()
        .stderr
        .clone();
    let err = String::from_utf8(out).unwrap();
    assert!(err.contains("ownr") && err.contains("system.toml"), "{err}");
}

#[test]
fn bad_version_provider_ref_and_duplicate_exit_3() {
    for bad in [
        GOOD.replace("version = 1", "version = 2"),
        GOOD.replace("provider = \"npm\"\nkind", "provider = \"nmp\"\nkind"),
        GOOD.replace("id = \"npm-publish\"", "id = \"site-deploy\""),
        GOOD.replace("max_ttl = \"90d\"", "max_ttl = \"90 days\""),
        GOOD.replace("expires_at = \"2027-01-01\"", "expires_at = \"2027-13-01\""),
        GOOD.replace(
            "kind = \"gh-env-secret\", repo = \"acme/app\", env = \"site\", key = \"NPM_TOKEN\"",
            "kind = \"vault\", key = \"NPM_TOKEN\"",
        ),
    ] {
        let d = repo(Some(&bad));
        run(d.path(), &["system", "check"], 3);
    }
    let d = repo(None);
    run(d.path(), &["system", "check"], 3);
}

#[test]
fn undeclared_and_unused_are_drift() {
    let sys = GOOD.replace("key = \"NPM_TOKEN\"", "key = \"OLD_NPM_TOKEN\"");
    let d = repo(Some(&sys));
    let r = run(d.path(), &["system", "check"], 1);
    let und = items(&r, "undeclared");
    assert_eq!(und.len(), 1, "{r}");
    assert_eq!(und[0]["key"], "NPM_TOKEN");
    assert_eq!(und[0]["action"], "drift");
    assert!(
        und[0]["source"]
            .as_str()
            .unwrap()
            .starts_with(".github/workflows/deploy.yml:")
    );
    // GITHUB_TOKEN is never a credential.
    assert!(!r.to_string().contains("\"GITHUB_TOKEN\""));
    let unused = items(&r, "unused");
    assert_eq!(unused.len(), 1, "{r}");
    assert_eq!(unused[0]["key"], "npm-publish");
}

#[test]
fn consumer_mismatch_is_drift() {
    let sys = GOOD.replacen("job = \"deploy\"", "job = \"build\"", 1);
    let d = repo(Some(&sys));
    let r = run(d.path(), &["system", "check"], 1);
    let c = items(&r, "consumer");
    assert_eq!(
        c.len(),
        2,
        "listed but not reading, reading but not listed: {r}"
    );
}

#[test]
fn expiry_soon_is_a_human_step_expired_is_drift() {
    let soon = GOOD.replace("expires_at = \"2027-01-20\"", "expires_at = \"2026-12-20\"");
    let d = repo(Some(&soon));
    let r = run(d.path(), &["system", "check"], 2);
    let e = items(&r, "expiry");
    assert_eq!(e[0]["action"], "pending");
    assert_eq!(e[0]["reason"], "expires in 23 days");
    assert_eq!(r["human_steps"][0]["id"], "rotate:site-deploy");

    let gone = GOOD.replace("expires_at = \"2027-01-20\"", "expires_at = \"2026-11-20\"");
    let d = repo(Some(&gone));
    let r = run(d.path(), &["system", "check"], 1);
    assert_eq!(items(&r, "expiry")[0]["action"], "drift");
    assert_eq!(items(&r, "expiry")[0]["reason"], "expired 7 days ago");
}

#[test]
fn rotation_due_is_pending() {
    let sys = GOOD.replace(
        "max_ttl = \"90d\"",
        "max_ttl = \"90d\"\nrotate_every = \"2w\"",
    );
    let d = repo(Some(&sys));
    let r = run(d.path(), &["system", "check"], 2);
    assert_eq!(items(&r, "rotation")[0]["key"], "site-deploy");
}

#[test]
fn missing_metadata_and_ttl_are_drift() {
    let sys = GOOD
        .replace("purpose = \"Deploy the site\"", "purpose = \"TODO\"")
        .replace("expires_at = \"2027-01-20\"", "expires_at = \"2027-06-01\"");
    let d = repo(Some(&sys));
    let r = run(d.path(), &["system", "check"], 1);
    assert_eq!(items(&r, "metadata").len(), 1, "{r}");
    assert_eq!(items(&r, "max_ttl").len(), 1, "{r}");
    let never = GOOD.replace("expires_at = \"2027-01-20\"", "expires_at = \"never\"");
    let d = repo(Some(&never));
    let r = run(d.path(), &["system", "check"], 1);
    assert_eq!(items(&r, "max_ttl").len(), 1, "{r}");
    let never_no_created = never.replacen("created_at = \"2026-11-01\"\n", "", 1);
    let d = repo(Some(&never_no_created));
    let r = run(d.path(), &["system", "check"], 1);
    assert_eq!(items(&r, "max_ttl").len(), 1, "{r}");
    let none = GOOD.replace("expires_at = \"2027-01-20\"\n", "");
    let d = repo(Some(&none));
    let r = run(d.path(), &["system", "check"], 1);
    assert!(
        items(&r, "metadata")[0]["reason"]
            .as_str()
            .unwrap()
            .contains("expires_at")
    );
}

#[test]
fn long_lived_publish_token_without_exemption_is_keyless_drift() {
    let sys = GOOD.replace("keyless_exempt = \"first publish needs a token\"\n", "");
    let d = repo(Some(&sys));
    let r = run(d.path(), &["system", "check"], 1);
    let k = items(&r, "keyless");
    assert_eq!(k.len(), 1, "{r}");
    assert!(
        k[0]["reason"]
            .as_str()
            .unwrap()
            .contains("trusted publishing")
    );
}

#[test]
fn trust_replacing_an_active_role_is_keyless_drift() {
    let sys = format!(
        "{GOOD}\n[[trust]]\nprovider = \"npm\"\nkind = \"oidc-trusted-publishing\"\nsubject = \"acme/app:release.yml\"\nreplaces = \"publish\"\n"
    );
    let d = repo(Some(&sys));
    let r = run(d.path(), &["system", "check"], 1);
    assert!(
        items(&r, "keyless")[0]["reason"]
            .as_str()
            .unwrap()
            .contains("replaced")
    );
}

#[test]
fn file_flag_names_the_real_path_and_credential_line() {
    let d = repo(None);
    fs::write(d.path().join("other.toml"), GOOD).unwrap();
    let r = run(d.path(), &["system", "check", "--file", "other.toml"], 0);
    assert_eq!(items(&r, "credential")[0]["source"], "other.toml:20", "{r}");
}

#[test]
fn live_without_tokens_is_unknown_not_a_failure() {
    let d = repo(Some(GOOD));
    let r = run(d.path(), &["system", "check", "--live"], 0);
    let l = items(&r, "live");
    // One per credential plus the GitHub storage sweep (no GitHub token).
    let keys: Vec<&str> = l.iter().map(|i| i["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["site-deploy", "npm-publish", "github"], "{r}");
    assert!(l.iter().all(|i| i["action"] == "unknown"));
}

#[test]
fn init_drafts_from_workflows_and_never_overwrites() {
    let d = repo(None);
    let r = run(d.path(), &["system", "init"], 0);
    assert_eq!(r["command"], "system init");
    assert_eq!(r["items"][0]["action"], "create");
    let text = fs::read_to_string(d.path().join("system.toml")).unwrap();
    assert!(text.contains("key = \"CLOUDFLARE_API_TOKEN\""), "{text}");
    assert!(text.contains("key = \"NPM_TOKEN\""), "{text}");
    assert!(text.contains("purpose = \"TODO\""), "{text}");
    assert!(!text.contains("GITHUB_TOKEN"), "{text}");
    // The draft parses and every workflow secret is declared.
    let r = run(d.path(), &["system", "check"], 1);
    assert!(items(&r, "undeclared").is_empty(), "{r}");
    assert!(items(&r, "consumer").is_empty(), "{r}");
    // Never overwritten.
    fs::write(d.path().join("system.toml"), GOOD).unwrap();
    let r = run(d.path(), &["system", "init"], 0);
    assert_eq!(r["items"][0]["action"], "ok");
    assert_eq!(
        fs::read_to_string(d.path().join("system.toml")).unwrap(),
        GOOD
    );
    // --print prints the draft and writes nothing.
    let out = common::hoist()
        .arg("-C")
        .arg(d.path())
        .args(["system", "init", "--print"])
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8(out).unwrap().contains("version = 1"));
    assert_eq!(
        fs::read_to_string(d.path().join("system.toml")).unwrap(),
        GOOD
    );
}

#[test]
fn explain_is_deterministic_and_write_check_round_trip() {
    let d = repo(Some(GOOD));
    let md = |d: &Path| {
        common::hoist()
            .arg("-C")
            .arg(d)
            .args(["system", "explain"])
            .assert()
            .code(0)
            .get_output()
            .stdout
            .clone()
    };
    let a = md(d.path());
    assert_eq!(a, md(d.path()));
    let s = String::from_utf8(a).unwrap();
    assert!(s.contains("```mermaid"), "{s}");
    assert!(s.contains("| site-deploy |"), "{s}");
    assert!(
        s.contains("hoist secrets rotate CLOUDFLARE_API_TOKEN"),
        "{s}"
    );
    run(d.path(), &["system", "explain", "--check"], 1);
    let r = run(d.path(), &["system", "explain", "--write"], 0);
    assert_eq!(r["items"][0]["action"], "create");
    assert_eq!(fs::read_to_string(d.path().join("SYSTEM.md")).unwrap(), s);
    run(d.path(), &["system", "explain", "--check"], 0);
    let r = run(d.path(), &["system", "explain", "--write"], 0);
    assert_eq!(r["items"][0]["action"], "ok");
}

/// Keys the schema allows at each level, compared with the Rust model through a full file.
#[test]
fn schema_is_json_and_closed() {
    let text = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/schema/system.schema.json"
    ))
    .unwrap();
    let schema: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    let doc: toml::Value = toml::from_str(GOOD).unwrap();
    assert_conforms(&schema, &schema, &serde_json::to_value(doc).unwrap(), "$");
}

fn resolve<'a>(root: &'a Value, s: &'a Value) -> &'a Value {
    match s["$ref"].as_str() {
        Some(r) => root
            .pointer(r.trim_start_matches('#'))
            .expect("ref resolves"),
        None => s,
    }
}

fn assert_conforms(root: &Value, schema: &Value, v: &Value, at: &str) {
    let s = resolve(root, schema);
    if let Some(any) = s["oneOf"].as_array().or(s["anyOf"].as_array()) {
        assert!(
            any.iter()
                .any(|alt| std::panic::catch_unwind(|| assert_conforms(root, alt, v, at)).is_ok()),
            "{at}: no alternative matches"
        );
        return;
    }
    match v {
        Value::Object(m) => {
            let props = s["properties"]
                .as_object()
                .unwrap_or_else(|| panic!("{at}: no properties"));
            for (k, val) in m {
                let p = props
                    .get(k)
                    .unwrap_or_else(|| panic!("{at}.{k}: not in schema"));
                assert_conforms(root, p, val, &format!("{at}.{k}"));
            }
            for r in s["required"].as_array().into_iter().flatten() {
                assert!(m.contains_key(r.as_str().unwrap()), "{at}: missing {r}");
            }
        }
        Value::Array(a) => a
            .iter()
            .for_each(|x| assert_conforms(root, &s["items"], x, at)),
        _ => {}
    }
}

#[test]
fn keelstack_system_toml_checks_offline_and_system_md_is_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let out = common::hoist()
        .env("HOIST_NOW", NOW)
        .arg("-C")
        .arg(root)
        .args(["system", "check", "--json"])
        .output()
        .unwrap();
    let r: Value = serde_json::from_slice(&out.stdout).expect("one JSON object");
    assert!(items(&r, "undeclared").is_empty(), "{r}");
    assert!(items(&r, "consumer").is_empty(), "{r}");
    assert!(items(&r, "unused").is_empty(), "{r}");
    common::hoist()
        .arg("-C")
        .arg(root)
        .args([
            "system",
            "explain",
            "--check",
            "--catalog",
            "decisions/channel/pieces/catalog/files/catalog.toml",
        ])
        .assert()
        .code(0);
    let text = fs::read_to_string(root.join("system.toml")).unwrap();
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(root.join("hoist/schema/system.schema.json")).unwrap(),
    )
    .unwrap();
    let doc: toml::Value = toml::from_str(&text).unwrap();
    assert_conforms(&schema, &schema, &serde_json::to_value(doc).unwrap(), "$");
}
