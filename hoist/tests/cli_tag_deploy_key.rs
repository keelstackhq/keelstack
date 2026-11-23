//! `[github] tags_bypass = "deploy_key"` (spec §5, §6): the `hoist: tags` ruleset's only
//! bypass is the repo's write deploy key `hoist: release tags`, whose private key hoist seals
//! into the release environment's secret `RELEASE_TAG_KEY`. Fixture repo plus the fake API.

mod common;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{FakeApi, git_repo, hoist};
use crypto_box::SecretKey;
use crypto_box::aead::OsRng;
use serde_json::{Value, json};
use std::path::Path;

const TOML: &str =
    "[github]\ntags_bypass = \"deploy_key\"\n\n[github.tag_ruleset]\npatterns = [\"v*\"]\n";

const KEYS: &str = "GET /repos/acme/x/keys";
const ENV_SECRETS: &str = "GET /repos/acme/x/environments/release/secrets";
const ENV_KEY: &str = "GET /repos/acme/x/environments/release/secrets/public-key";
const PUT_SECRET: &str = "PUT /repos/acme/x/environments/release/secrets/RELEASE_TAG_KEY";
const POST_KEY: &str = "POST /repos/acme/x/keys";
const LIST_TAGS: &str = r#"[{"id":12,"name":"hoist: tags"}]"#;
const RULES: &str = r#"[{"type":"creation"},{"type":"update"},{"type":"deletion"}]"#;
const OURS: &str =
    r#"{"id":77,"title":"hoist: release tags","read_only":false,"key":"ssh-ed25519 AAAA"}"#;
const SECRET_THERE: &str = r#"{"total_count":1,"secrets":[{"name":"RELEASE_TAG_KEY","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"}]}"#;
const SECRET_NONE: &str = r#"{"total_count":0,"secrets":[]}"#;
const DEPLOY_KEY_ACTOR: &str =
    r#"{"actor_id":null,"actor_type":"DeployKey","bypass_mode":"always"}"#;
const ACTIONS_ACTOR: &str =
    r#"{"actor_id":15368,"actor_type":"Integration","bypass_mode":"always"}"#;
const TEAM_ACTOR: &str = r#"{"actor_id":7,"actor_type":"Team","bypass_mode":"always"}"#;

fn tags(bypass: &str) -> String {
    format!(
        r#"{{"id":12,"name":"hoist: tags","target":"tag","enforcement":"active","conditions":{{"ref_name":{{"include":["refs/tags/v*"],"exclude":[]}}}},"rules":{RULES},"bypass_actors":[{bypass}]}}"#
    )
}

/// Route list: `set` replaces a key's responses.
struct Routes(Vec<(String, u16, String)>);

impl Routes {
    /// No deploy key, no secret, the tag ruleset still bypassed by the Actions app and a team.
    fn fresh(sealing_key: &str) -> Routes {
        let mut r = Routes(Vec::new());
        for (k, s, b) in [
            (
                "GET /repos/acme/x",
                200,
                "github/repo_admin.json".to_string(),
            ),
            ("GET /repos/acme/x/rulesets", 200, LIST_TAGS.to_string()),
            (
                "GET /repos/acme/x/rulesets/12",
                200,
                tags(&format!("{ACTIONS_ACTOR},{TEAM_ACTOR},{ACTIONS_ACTOR}")),
            ),
            (
                "GET /repos/acme/x/actions/permissions/workflow",
                200,
                "github/actions_permissions.json".to_string(),
            ),
            (
                "GET /repos/acme/x/environments",
                200,
                r#"{"total_count":1,"environments":[{"name":"release"}]}"#.to_string(),
            ),
            (KEYS, 200, "[]".to_string()),
            (ENV_SECRETS, 200, SECRET_NONE.to_string()),
            (ENV_KEY, 200, sealing_key.to_string()),
            (
                POST_KEY,
                201,
                r#"{"id":77,"title":"hoist: release tags","read_only":false}"#.to_string(),
            ),
            (PUT_SECRET, 201, "{}".to_string()),
            ("PUT /repos/acme/x/rulesets/12", 200, "{}".to_string()),
            ("DELETE /repos/acme/x/keys/77", 204, String::new()),
        ] {
            r.0.push((k.to_string(), s, b));
        }
        r
    }

    fn set(mut self, key: &str, status: u16, body: &str) -> Routes {
        self.0.retain(|(k, ..)| k != key);
        self.0.push((key.to_string(), status, body.to_string()));
        self
    }

    /// Append the next response for a key (route lists answer in order, the last repeats).
    fn then(mut self, key: &str, status: u16, body: &str) -> Routes {
        self.0.push((key.to_string(), status, body.to_string()));
        self
    }

    fn start(&self) -> FakeApi {
        let specs: Vec<common::RouteSpec> = self
            .0
            .iter()
            .map(|(k, s, b)| (k.as_str(), *s, b.as_str(), &[][..]))
            .collect();
        FakeApi::start(&specs)
    }
}

fn keypair() -> (SecretKey, String) {
    let sk = SecretKey::generate(&mut OsRng);
    let b64 = STANDARD.encode(sk.public_key().as_bytes());
    (sk, format!(r#"{{"key_id":"kid-1","key":"{b64}"}}"#))
}

struct Out {
    report: Value,
    stdout: String,
    stderr: String,
}

fn run(api: &FakeApi, dir: &Path, args: &[&str], code: i32) -> Out {
    let out = hoist()
        .current_dir(dir)
        .env("GITHUB_TOKEN", "gh-test-token")
        .env("HOIST_GITHUB_API", api.url())
        .arg("--json")
        .args(args)
        .assert()
        .code(code)
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let report =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout}\n{stderr}"));
    Out {
        report,
        stdout,
        stderr,
    }
}

fn repo(toml: &str) -> tempfile::TempDir {
    let dir = git_repo();
    std::fs::write(dir.path().join("hoist.toml"), toml).unwrap();
    dir
}

fn item<'a>(report: &'a Value, kind: &str, key: &str) -> &'a Value {
    report["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == kind && i["key"] == key)
        .unwrap_or_else(|| panic!("no {kind} {key} in {report}"))
}

fn writes(api: &FakeApi) -> Vec<String> {
    api.requests()
        .iter()
        .filter(|r| r.method != "GET")
        .map(|r| format!("{} {}", r.method, r.path))
        .collect()
}

fn body_of(api: &FakeApi, method: &str, path: &str) -> Value {
    let r = api
        .requests()
        .into_iter()
        .find(|r| r.method == method && r.path == path)
        .unwrap_or_else(|| panic!("no {method} {path}"));
    serde_json::from_str(r.body.as_deref().unwrap()).unwrap()
}

/// The openssh-key-v1 fields of an unencrypted private key: (public blob, private seed+public).
fn parse_openssh(pem: &str) -> (Vec<u8>, Vec<u8>) {
    let lines: Vec<&str> = pem.lines().collect();
    assert_eq!(lines.first(), Some(&"-----BEGIN OPENSSH PRIVATE KEY-----"));
    assert_eq!(lines.last(), Some(&"-----END OPENSSH PRIVATE KEY-----"));
    assert!(pem.ends_with('\n'));
    let inner = &lines[1..lines.len() - 1];
    assert!(inner.iter().all(|l| l.len() <= 70), "lines wrap at 70");
    let raw = STANDARD.decode(inner.concat()).unwrap();
    let magic = b"openssh-key-v1\0";
    assert_eq!(&raw[..magic.len()], magic);
    let mut at = magic.len();
    assert_eq!(take_at(&raw, &mut at), b"none", "cipher");
    assert_eq!(take_at(&raw, &mut at), b"none", "kdf");
    assert_eq!(take_at(&raw, &mut at), b"", "kdf options");
    assert_eq!(take_count(&raw, &mut at), 1, "one key");
    let public_blob = take_at(&raw, &mut at);
    let private = take_at(&raw, &mut at);
    assert_eq!(at, raw.len());
    assert_eq!(private.len() % 8, 0, "padded to the cipher block size 8");
    let mut p = 0;
    let c1 = &private[0..4];
    let c2 = &private[4..8];
    assert_eq!(c1, c2, "check ints match");
    p += 8;
    assert_eq!(take_at(&private, &mut p), b"ssh-ed25519");
    let public = take_at(&private, &mut p);
    let secret = take_at(&private, &mut p);
    let _comment = take_at(&private, &mut p);
    for (i, b) in private[p..].iter().enumerate() {
        assert_eq!(*b as usize, i + 1, "padding 1, 2, 3, ...");
    }
    assert_eq!(public.len(), 32);
    assert_eq!(secret.len(), 64);
    assert_eq!(&secret[32..], &public[..], "private = seed || public");
    let mut want_blob = Vec::new();
    for part in [&b"ssh-ed25519"[..], &public[..]] {
        want_blob.extend((part.len() as u32).to_be_bytes());
        want_blob.extend(part);
    }
    assert_eq!(public_blob, want_blob);
    (public_blob, secret)
}

fn take_count(raw: &[u8], at: &mut usize) -> u32 {
    let n = u32::from_be_bytes(raw[*at..*at + 4].try_into().unwrap());
    *at += 4;
    n
}

fn take_at(raw: &[u8], at: &mut usize) -> Vec<u8> {
    let n = u32::from_be_bytes(raw[*at..*at + 4].try_into().unwrap()) as usize;
    let v = raw[*at + 4..*at + 4 + n].to_vec();
    *at += 4 + n;
    v
}

#[test]
fn plan_and_check_report_a_missing_key_get_only() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key).start();
    let dir = repo(TOML);
    let plan = run(&api, dir.path(), &["plan", "--area", "github"], 0).report;
    let k = item(&plan, "tag_deploy_key", "hoist: release tags");
    assert_eq!(k["action"], "create", "{k}");
    assert!(
        k["reason"].as_str().unwrap().contains("RELEASE_TAG_KEY"),
        "{k}"
    );
    let t = item(&plan, "tag_ruleset", "hoist: tags");
    assert_eq!(t["action"], "update", "{t}");
    assert!(t["reason"].as_str().unwrap().contains("deploy key"), "{t}");
    run(&api, dir.path(), &["check", "--area", "github"], 1);
    api.assert_only_get();
    assert!(
        api.requests()
            .iter()
            .any(|r| r.path == "/repos/acme/x/keys")
    );
}

#[test]
fn apply_adds_a_write_key_seals_the_private_key_and_bypasses_only_deploy_keys() {
    let (sk, key) = keypair();
    let api = Routes::fresh(&key).start();
    let dir = repo(TOML);
    let out = run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 1);
    let w = writes(&api);
    let pos = |s: &str| {
        w.iter()
            .position(|x| x == s)
            .unwrap_or_else(|| panic!("{s}: {w:?}"))
    };
    assert!(pos(POST_KEY) < pos(PUT_SECRET), "{w:?}");
    assert!(
        pos(PUT_SECRET) < pos("PUT /repos/acme/x/rulesets/12"),
        "{w:?}"
    );

    let post = body_of(&api, "POST", "/repos/acme/x/keys");
    assert_eq!(post["title"], "hoist: release tags");
    assert_eq!(post["read_only"], false);
    let line = post["key"].as_str().unwrap();
    let b64 = line
        .strip_prefix("ssh-ed25519 ")
        .expect("an OpenSSH public key line");
    let blob = STANDARD.decode(b64.split(' ').next().unwrap()).unwrap();

    let put = body_of(
        &api,
        "PUT",
        "/repos/acme/x/environments/release/secrets/RELEASE_TAG_KEY",
    );
    assert_eq!(put["key_id"], "kid-1");
    let sealed = STANDARD
        .decode(put["encrypted_value"].as_str().unwrap())
        .unwrap();
    let pem = String::from_utf8(sk.unseal(&sealed).expect("sealed for the env key")).unwrap();
    let (public_blob, _) = parse_openssh(&pem);
    assert_eq!(
        public_blob, blob,
        "the secret holds the key the deploy key names"
    );

    let rs = body_of(&api, "PUT", "/repos/acme/x/rulesets/12");
    assert_eq!(
        rs["bypass_actors"],
        json!([
            {"actor_id": 7, "actor_type": "Team", "bypass_mode": "always"},
            {"actor_id": null, "actor_type": "DeployKey", "bypass_mode": "always"}
        ]),
        "the Actions app hoist added is gone, the owner's team stays"
    );

    // No key material anywhere but the sealed body.
    let middle = pem.lines().nth(2).unwrap();
    for text in [&out.stdout, &out.stderr] {
        assert!(!text.contains("OPENSSH PRIVATE KEY"), "{text}");
        assert!(!text.contains(middle), "{text}");
    }
    for r in api.requests() {
        let b = r.body.unwrap_or_default();
        assert!(!b.contains("OPENSSH"), "{} {}", r.method, r.path);
        assert!(!b.contains(middle), "{} {}", r.method, r.path);
    }
}

#[test]
fn a_failed_seal_deletes_the_key_just_added() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key)
        .set(PUT_SECRET, 500, r#"{"message":"boom"}"#)
        .start();
    let dir = repo(TOML);
    run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 1);
    let w = writes(&api);
    assert!(
        w.contains(&"DELETE /repos/acme/x/keys/77".to_string()),
        "{w:?}"
    );
    assert!(
        !w.contains(&"PUT /repos/acme/x/rulesets/12".to_string()),
        "{w:?}"
    );
}

#[test]
fn an_existing_key_and_secret_are_kept_on_a_rerun() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key)
        .set(KEYS, 200, &format!("[{OURS}]"))
        .set(ENV_SECRETS, 200, SECRET_THERE)
        .set(
            "GET /repos/acme/x/rulesets/12",
            200,
            &tags(&format!("{TEAM_ACTOR},{DEPLOY_KEY_ACTOR}")),
        )
        .start();
    let dir = repo(TOML);
    let plan = run(&api, dir.path(), &["plan", "--area", "github"], 0).report;
    assert_eq!(
        item(&plan, "tag_deploy_key", "hoist: release tags")["action"],
        "ok"
    );
    let t = item(&plan, "tag_ruleset", "hoist: tags");
    assert_eq!(t["action"], "ok", "{t}");
    run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 0);
    assert!(writes(&api).is_empty(), "{:?}", writes(&api));
}

#[test]
fn a_key_without_its_secret_or_a_secret_without_its_key_is_drift() {
    let (_sk, key) = keypair();
    let dir = repo(TOML);
    for (keys, secrets, says) in [
        (format!("[{OURS}]"), SECRET_NONE, "delete the deploy key"),
        ("[]".to_string(), SECRET_THERE, "delete the secret"),
    ] {
        let api = Routes::fresh(&key)
            .set(KEYS, 200, &keys)
            .set(ENV_SECRETS, 200, secrets)
            .set(
                "GET /repos/acme/x/rulesets/12",
                200,
                &tags(DEPLOY_KEY_ACTOR),
            )
            .start();
        let plan = run(&api, dir.path(), &["plan", "--area", "github"], 0).report;
        let k = item(&plan, "tag_deploy_key", "hoist: release tags");
        assert_eq!(k["action"], "drift", "{k}");
        assert!(k["reason"].as_str().unwrap().contains(says), "{k}");
        run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 1);
        assert!(writes(&api).is_empty(), "never guesses: {:?}", writes(&api));
    }
}

#[test]
fn a_read_only_key_or_another_write_key_is_drift() {
    let (_sk, key) = keypair();
    let dir = repo(TOML);
    let read_only = OURS.replace(r#""read_only":false"#, r#""read_only":true"#);
    let other = r#"{"id":5,"title":"ci deploy","read_only":false,"key":"ssh-ed25519 BBBB"}"#;
    for (keys, says) in [
        (format!("[{read_only}]"), "read-only"),
        (format!("[{OURS},{other}]"), "`ci deploy`"),
    ] {
        let api = Routes::fresh(&key)
            .set(KEYS, 200, &keys)
            .set(ENV_SECRETS, 200, SECRET_THERE)
            .set(
                "GET /repos/acme/x/rulesets/12",
                200,
                &tags(DEPLOY_KEY_ACTOR),
            )
            .start();
        let plan = run(&api, dir.path(), &["plan", "--area", "github"], 0).report;
        let k = item(&plan, "tag_deploy_key", "hoist: release tags");
        assert_eq!(k["action"], "drift", "{k}");
        assert!(k["reason"].as_str().unwrap().contains(says), "{k}");
    }
}

#[test]
fn a_missing_release_environment_counts_as_no_secret() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key)
        .set(ENV_SECRETS, 404, r#"{"message":"Not Found"}"#)
        .start();
    let dir = repo(TOML);
    let plan = run(&api, dir.path(), &["plan", "--area", "github"], 0).report;
    assert_eq!(
        item(&plan, "tag_deploy_key", "hoist: release tags")["action"],
        "create"
    );
}

#[test]
fn bad_tags_bypass_settings_exit_3() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key).start();
    for toml in [
        "[github]\ntags_bypass = \"bogus\"\n\n[github.tag_ruleset]\npatterns = [\"v*\"]\nbypass = [\"github-actions\"]\n",
        "[github]\ntags_bypass = \"deploy_key\"\n",
        "[github]\ntags_bypass = \"deploy_key\"\n\n[github.tag_ruleset]\npatterns = [\"v*\"]\nbypass = [\"github-actions\"]\n",
    ] {
        let dir = repo(toml);
        let out = run(&api, dir.path(), &["plan", "--area", "github"], 3);
        assert!(
            out.report["error"]["message"]
                .as_str()
                .unwrap()
                .contains("tags_bypass")
                || out.stderr.contains("tags_bypass"),
            "{toml}: {}",
            out.stderr
        );
    }
}

#[test]
fn actions_mode_dedupes_the_actions_app() {
    // The default (`actions`): a live ruleset listing the app twice is rewritten with it once.
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key).start();
    let dir = repo("[github.tag_ruleset]\npatterns = [\"v*\"]\nbypass = [\"github-actions\"]\n");
    let plan = run(&api, dir.path(), &["plan", "--area", "github"], 0).report;
    let t = item(&plan, "tag_ruleset", "hoist: tags");
    assert_eq!(t["action"], "update", "{t}");
    assert!(
        !plan["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "tag_deploy_key"),
        "no deploy key in actions mode"
    );
    run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 1);
    assert!(
        !api.requests()
            .iter()
            .any(|r| r.path == "/repos/acme/x/keys")
    );
    let rs = body_of(&api, "PUT", "/repos/acme/x/rulesets/12");
    assert_eq!(
        rs["bypass_actors"],
        json!([
            {"actor_id": 15368, "actor_type": "Integration", "bypass_mode": "always"},
            {"actor_id": 7, "actor_type": "Team", "bypass_mode": "always"}
        ])
    );
}

#[test]
fn disabled_deploy_keys_are_a_human_step_not_a_failure() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key)
        .set(
            POST_KEY,
            422,
            r#"{"message":"Validation Failed","errors":[{"resource":"PublicKey","code":"custom","message":"Deploy keys are disabled for this repository"}],"status":"422"}"#,
        )
        .start();
    let dir = repo(TOML);
    let out = run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 2);
    let step = out.report["human_steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "deploy-keys-policy")
        .unwrap_or_else(|| panic!("no step in {}", out.report));
    assert_eq!(
        step["title"],
        "allow deploy keys for acme/x (organization settings), then re-run keel up"
    );
    assert_eq!(
        step["url"],
        "https://github.com/organizations/acme/settings"
    );
    assert_eq!(step["status"], "pending");
    for (kind, key) in [
        ("tag_deploy_key", "hoist: release tags"),
        ("tag_ruleset", "hoist: tags"),
    ] {
        let i = item(&out.report, kind, key);
        assert_eq!(i["action"], "pending", "{i}");
        assert!(
            i["reason"].as_str().unwrap().contains("allow deploy keys"),
            "{i}"
        );
    }
    let w = writes(&api);
    assert_eq!(
        w,
        vec![POST_KEY.to_string()],
        "no ruleset, no secret: {w:?}"
    );
}

#[test]
fn a_non_policy_key_rejection_fails_with_its_errors_and_stops() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key)
        .set(
            POST_KEY,
            422,
            r#"{"message":"Validation Failed","errors":[{"resource":"Key","code":"custom","field":"key","message":"key is already in use"}]}"#,
        )
        .start();
    let dir = repo(TOML);
    let out = run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 1);
    let k = item(&out.report, "tag_deploy_key", "hoist: release tags");
    let reason = k["reason"].as_str().unwrap();
    assert!(reason.contains("key is already in use"), "{k}");
    assert!(reason.contains("field key"), "{k}");
    assert_eq!(
        item(&out.report, "tag_ruleset", "hoist: tags")["reason"],
        "not attempted"
    );
    assert!(out.report["human_steps"].as_array().unwrap().is_empty());
    assert_eq!(writes(&api), vec![POST_KEY.to_string()]);
}

#[test]
fn disabled_deploy_keys_still_apply_repo_settings_and_actions_permissions() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key)
        .set(
            "GET /repos/acme/x/actions/permissions/workflow",
            200,
            r#"{"default_workflow_permissions":"write","can_approve_pull_request_reviews":false}"#,
        )
        .set(
            "GET /repos/acme/x",
            200,
            r#"{"id":1,"name":"x","full_name":"acme/x","private":false,"visibility":"public","default_branch":"main","allow_auto_merge":false,"delete_branch_on_merge":true,"allow_squash_merge":true,"permissions":{"admin":true,"maintain":true,"push":true,"triage":true,"pull":true}}"#,
        )
        .then(
            "GET /repos/acme/x/actions/permissions/workflow",
            200,
            "github/actions_permissions.json",
        )
        .then("GET /repos/acme/x", 200, "github/repo_admin.json")
        .set("PATCH /repos/acme/x", 200, "{}")
        .set(
            "PUT /repos/acme/x/actions/permissions/workflow",
            204,
            "",
        )
        .set(
            POST_KEY,
            422,
            r#"{"message":"Validation Failed","errors":[{"resource":"PublicKey","code":"custom","message":"Deploy keys are disabled for this repository"}],"status":"422"}"#,
        )
        .start();
    let dir = repo(TOML);
    let out = run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 2);
    let w = writes(&api);
    for want in [
        "PATCH /repos/acme/x",
        "PUT /repos/acme/x/actions/permissions/workflow",
        POST_KEY,
    ] {
        assert!(w.contains(&want.to_string()), "{want}: {w:?}");
    }
    assert!(
        !w.iter()
            .any(|x| x.contains("/rulesets") || x.starts_with("PUT") && x.contains("/secrets/")),
        "{w:?}"
    );
    for (kind, key) in [
        ("tag_deploy_key", "hoist: release tags"),
        ("tag_ruleset", "hoist: tags"),
    ] {
        assert_eq!(item(&out.report, kind, key)["action"], "pending");
    }
    assert!(
        out.report["human_steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == "deploy-keys-policy")
    );
}

const POLICY_422: &str = r#"{"message":"Validation Failed","errors":[{"resource":"PublicKey","code":"custom","message":"Deploy keys are disabled for this repository"}],"status":"422"}"#;

#[test]
fn a_plan_limited_tag_ruleset_stays_pending_when_deploy_keys_are_disabled() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key)
        .set(
            "GET /repos/acme/x/rulesets",
            403,
            r#"{"message":"Upgrade to GitHub Pro or make this repository public to enable this feature."}"#,
        )
        .set(POST_KEY, 422, POLICY_422)
        .start();
    let dir = repo(TOML);
    let out = run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 2);
    assert_eq!(
        item(&out.report, "tag_ruleset", "hoist: tags")["action"],
        "pending"
    );
    assert_eq!(
        item(&out.report, "tag_deploy_key", "hoist: release tags")["action"],
        "pending"
    );
    assert!(
        !writes(&api).iter().any(|w| w.contains("/rulesets")),
        "{:?}",
        writes(&api)
    );
}

#[test]
fn a_failed_write_after_the_policy_step_stops_and_the_tag_items_stay_pending() {
    let (_sk, key) = keypair();
    let api = Routes::fresh(&key)
        .set(
            "GET /repos/acme/x",
            200,
            r#"{"id":1,"name":"x","full_name":"acme/x","private":false,"visibility":"public","default_branch":"main","allow_auto_merge":false,"delete_branch_on_merge":true,"allow_squash_merge":true,"permissions":{"admin":true,"maintain":true,"push":true,"triage":true,"pull":true}}"#,
        )
        .set(
            "GET /repos/acme/x/actions/permissions/workflow",
            200,
            r#"{"default_workflow_permissions":"write","can_approve_pull_request_reviews":false}"#,
        )
        .set(POST_KEY, 422, POLICY_422)
        .set("PATCH /repos/acme/x", 500, r#"{"message":"boom"}"#)
        .start();
    let dir = repo(TOML);
    let out = run(&api, dir.path(), &["apply", "--yes", "--area", "github"], 1);
    for (kind, key) in [
        ("tag_deploy_key", "hoist: release tags"),
        ("tag_ruleset", "hoist: tags"),
    ] {
        assert_eq!(item(&out.report, kind, key)["action"], "pending", "{kind}");
    }
    assert!(
        item(&out.report, "repo_setting", "allow_auto_merge")["reason"]
            .as_str()
            .unwrap()
            .starts_with("failed")
    );
    assert_eq!(
        item(&out.report, "actions_permissions", "default")["reason"],
        "not attempted"
    );
    let w = writes(&api);
    assert!(
        !w.iter()
            .any(|x| x.contains("/rulesets") || x.contains("actions/permissions")),
        "{w:?}"
    );
}
