//! `bosun owner setup` (B2 T2): environments, variable, sealed secrets, the signer key, the
//! push of `bosun/gate-app`, against the fake GitHub and a local bare remote.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use common::fake::{ENV_KEY_ID, FakeGithub, env_secret_key};
use common::{TOKEN_CANARY, bosun, one_object};
use serde_json::Value;

const KEY: &str = "-----BEGIN RSA PRIVATE KEY-----\nKEYMATERIALCANARY0123456789abcdef\n-----END RSA PRIVATE KEY-----\n";
const MANUAL: [&str; 10] = [
    "owner",
    "setup",
    "--manual",
    "--app-id",
    "777",
    "--client-id",
    "Iv1.fakeclient",
    "--slug",
    "acme-bosun-gate",
    "--key-stdin",
];
const APP_VAR: &str = "bosun-app/BOSUN_APP_CLIENT_ID";
const APP_KEY: &str = "bosun-app/BOSUN_APP_KEY";
const SIGNER_KEY: &str = "bosun-signer/BOSUN_SIGNER_KEY";

struct Env {
    fake: FakeGithub,
    repo: tempfile::TempDir,
    origin: tempfile::TempDir,
    home: tempfile::TempDir,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .unwrap()
        .success()
}

impl Env {
    fn new() -> Env {
        let fake = FakeGithub::start();
        {
            let mut s = fake.state.lock().unwrap();
            s.manifest_codes.push("c0de1234abcd5678ef90".into());
            s.manifest_pem = KEY.to_string();
        }
        let repo = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(repo.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(repo.path().join("README.md"), "hello\n").unwrap();
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "init"]);
        git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        git(repo.path(), &["push", "-q", "origin", "main"]);
        Env {
            fake,
            repo,
            origin,
            home: tempfile::tempdir().unwrap(),
        }
    }

    fn cmd(&self) -> assert_cmd::Command {
        let mut c = bosun();
        c.current_dir(self.repo.path())
            .env("BOSUN_GITHUB_API", self.fake.url())
            .env("HOME", self.home.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0");
        c
    }

    fn run(&self, args: &[&str], stdin: &str) -> (std::process::Output, Value) {
        let out = self.cmd().args(args).write_stdin(stdin).output().unwrap();
        let all = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!all.contains(TOKEN_CANARY), "token leaked");
        assert!(
            !all.contains("KEYMATERIALCANARY") && !all.contains("PRIVATE KEY"),
            "key material in output: {all}"
        );
        let v = one_object(&out.stdout);
        (out, v)
    }

    fn mutations(&self) -> Vec<String> {
        self.fake
            .requests()
            .into_iter()
            .filter(|r| r.method != "GET")
            .map(|r| format!("{} {}", r.method, r.path))
            .collect()
    }

    fn unseal(&self, key: &str) -> String {
        let sealed = self.fake.state.lock().unwrap().env_secrets[key].1.clone();
        let raw = bosun::github::b64::decode(&sealed).unwrap();
        String::from_utf8(env_secret_key().unseal(&raw).unwrap()).unwrap()
    }

    fn sealed(&self, key: &str) -> Option<(String, String)> {
        self.fake
            .state
            .lock()
            .unwrap()
            .env_secrets
            .get(key)
            .cloned()
    }

    fn remote_show(&self, path: &str) -> String {
        git(
            self.origin.path(),
            &["show", &format!("bosun/gate-app:{path}")],
        )
    }
}

#[test]
fn uc_b2_1_one_run_creates_everything_and_the_rerun_changes_nothing() {
    let e = Env::new();
    let main_before = git(e.origin.path(), &["rev-parse", "main"]);
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    assert_eq!(v["command"], "owner setup");
    assert_eq!(v["data"]["app_id"], 777);
    assert_eq!(v["data"]["slug"], "acme-bosun-gate");
    assert_eq!(v["data"]["branch"], "bosun/gate-app");
    assert_eq!(v["data"]["pushed"], true);

    // Both environments: branch policy main only, no reviewers.
    {
        let s = e.fake.state.lock().unwrap();
        for env in ["bosun-app", "bosun-signer"] {
            let (policy, branches) = &s.environments[env];
            assert_eq!(policy["custom_branch_policies"], true);
            assert_eq!(policy["protected_branches"], false);
            assert_eq!(branches.as_array().unwrap().len(), 1);
            assert_eq!(branches[0]["name"], "main");
            assert_eq!(s.env_reviewers[env], serde_json::json!([]));
        }
        assert_eq!(s.env_vars[APP_VAR], "Iv1.fakeclient");
    }
    // The secrets are sealed boxes of the right values.
    assert_eq!(e.unseal(APP_KEY), KEY);
    let signer_pem = e.unseal(SIGNER_KEY);
    assert!(signer_pem.starts_with("-----BEGIN PRIVATE KEY-----"));
    // The signer key in the secret is the one enrolled on the branch.
    let body: String = signer_pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let der = bosun::github::b64::decode(&body).unwrap();
    let pair = ring::signature::Ed25519KeyPair::from_pkcs8(&der).unwrap();
    let public = bosun::github::b64::encode(ring::signature::KeyPair::public_key(&pair).as_ref());
    assert_eq!(v["data"]["signer_public_key"], public.as_str());
    let approvers = e.remote_show(".bosun/approvers.toml");
    assert!(
        approvers.contains(&format!("ed25519 = \"{public}\"")),
        "{approvers}"
    );
    assert!(approvers.contains("role = \"signer\""), "{approvers}");
    assert_eq!(
        e.remote_show(".bosun/gate-app.toml"),
        "id = 777\nslug = \"acme-bosun-gate\"\n"
    );
    // The secrets endpoint only ever saw sealed values; nothing was merged or opened.
    for r in e.fake.requests() {
        let body = r.body.unwrap_or_default();
        assert!(
            !body.contains("KEYMATERIAL") && !body.contains("PRIVATE KEY"),
            "{}",
            r.path
        );
        if r.method == "PUT" && r.path.contains("/secrets/") {
            let b: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(b["key_id"], ENV_KEY_ID);
            assert_eq!(b.as_object().unwrap().len(), 2);
        }
    }
    assert!(
        e.mutations()
            .iter()
            .all(|m| !m.contains("/pulls") && !m.contains("/merge"))
    );
    assert_eq!(git(e.origin.path(), &["rev-parse", "main"]), main_before);

    // Second run: zero mutating calls, no new push, secrets untouched.
    let before = (e.sealed(APP_KEY), e.sealed(SIGNER_KEY));
    let calls = e.fake.requests().len();
    let tip = git(e.origin.path(), &["rev-parse", "bosun/gate-app"]);
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    let again: Vec<_> = e.fake.requests().split_off(calls);
    assert!(again.iter().all(|r| r.method == "GET"), "{again:?}");
    assert_eq!(v["data"]["pushed"], false);
    assert!(
        v["data"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["device"] == "owner-computer" && s["step"] != "approve-enrollment")
            .all(|s| s["status"] == "skipped"),
        "{v}"
    );
    assert_eq!((e.sealed(APP_KEY), e.sealed(SIGNER_KEY)), before);
    assert_eq!(git(e.origin.path(), &["rev-parse", "bosun/gate-app"]), tip);
    assert!(e.fake.state.lock().unwrap().manifest_attempts.is_empty());
}

fn scan(dir: &Path, needles: &[&str], hits: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan(&p, needles, hits);
        } else if let Ok(bytes) = std::fs::read(&p) {
            let text = String::from_utf8_lossy(&bytes);
            for n in needles {
                if text.contains(n) {
                    hits.push(format!("{} has {n}", p.display()));
                }
            }
        }
    }
}

#[test]
fn uc_b2_2_no_private_key_on_disk_or_in_output_after_the_run() {
    let e = Env::new();
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    let signer_pem = e.unseal(SIGNER_KEY);
    let signer_body = signer_pem.lines().nth(1).unwrap().to_string();
    let needles = [
        "BEGIN",
        "PRIVATE KEY",
        "KEYMATERIAL",
        signer_body.as_str(),
        "FAKE-CLIENT-SECRET",
    ];
    let mut hits = Vec::new();
    for d in [e.home.path(), e.repo.path(), e.origin.path()] {
        scan(d, &needles, &mut hits);
    }
    assert!(hits.is_empty(), "{hits:?}");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for n in needles {
        assert!(!all.contains(n), "output has {n}");
    }
    // No leftover temp index or key file in the repo's git dir.
    assert!(!e.repo.path().join(".git/bosun-gate-app.index").exists());
}

#[test]
fn an_error_after_the_key_is_read_leaks_nothing() {
    let e = Env::new();
    e.fake
        .state
        .lock()
        .unwrap()
        .fail_paths
        .push(("/secrets/public-key".into(), 403));
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(!out.status.success());
    assert_eq!(v["ok"], false);
    assert!(!v["error"]["fix"].as_str().unwrap().is_empty());
    assert!(e.fake.state.lock().unwrap().env_secrets.is_empty());
}

#[test]
fn a_run_cut_before_the_push_resumes_without_touching_what_exists() {
    let e = Env::new();
    // The remote is missing: everything on GitHub is set up, then the push fails.
    let mut args = MANUAL.to_vec();
    args.extend(["--remote", "nowhere"]);
    let (out, v) = e.run(&args, KEY);
    assert!(!out.status.success(), "{v}");
    assert_eq!(v["ok"], false);
    let secrets = (e.sealed(APP_KEY), e.sealed(SIGNER_KEY));
    assert!(secrets.0.is_some() && secrets.1.is_some());
    assert!(git_ok(
        e.repo.path(),
        &["rev-parse", "--verify", "-q", "refs/heads/bosun/gate-app"]
    ));
    assert!(!git_ok(
        e.origin.path(),
        &["rev-parse", "--verify", "-q", "bosun/gate-app"]
    ));

    // The rerun needs no App credentials: the local branch names the App.
    let calls = e.fake.requests().len();
    let (out, v) = e.run(&["owner", "setup"], "");
    assert!(out.status.success(), "{v}");
    assert_eq!(v["data"]["pushed"], true);
    let again: Vec<_> = e.fake.requests().split_off(calls);
    assert!(again.iter().all(|r| r.method == "GET"), "{again:?}");
    assert_eq!((e.sealed(APP_KEY), e.sealed(SIGNER_KEY)), secrets);
    assert!(e.fake.state.lock().unwrap().manifest_attempts.is_empty());
    assert_eq!(
        e.remote_show(".bosun/gate-app.toml"),
        "id = 777\nslug = \"acme-bosun-gate\"\n"
    );
}

#[test]
fn an_existing_correct_environment_and_variable_are_left_alone() {
    let e = Env::new();
    e.fake.environment(
        "bosun-app",
        serde_json::json!({"protected_branches": false, "custom_branch_policies": true}),
        serde_json::json!([{"name": "main", "type": "branch"}]),
    );
    e.fake
        .state
        .lock()
        .unwrap()
        .env_vars
        .insert(APP_VAR.into(), "Iv1.fakeclient".into());
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    let muts = e.mutations();
    assert!(
        !muts
            .iter()
            .any(|m| m.contains("/environments/bosun-app") && !m.contains("/secrets/")),
        "{muts:?}"
    );
    assert!(
        muts.iter()
            .any(|m| m.contains("/environments/bosun-signer"))
    );
    assert_eq!(e.unseal(APP_KEY), KEY);
}

#[test]
fn an_environment_with_other_branches_is_fixed_and_reviewers_are_removed() {
    let e = Env::new();
    e.fake.environment(
        "bosun-signer",
        serde_json::Value::Null,
        serde_json::json!([]),
    );
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    let s = e.fake.state.lock().unwrap();
    assert_eq!(s.environments["bosun-signer"].1[0]["name"], "main");
    assert_eq!(s.env_reviewers["bosun-signer"], serde_json::json!([]));
}

#[test]
fn dry_run_calls_nothing_mutating_and_lists_the_owner_steps() {
    let e = Env::new();
    let (out, v) = e.run(&["owner", "setup", "--dry-run"], "");
    assert!(out.status.success(), "{v}");
    assert_eq!(v["data"]["dry_run"], true);
    let steps = v["data"]["steps"].as_array().unwrap();
    assert!(steps.len() >= 6, "{v}");
    for s in steps {
        let device = s["device"].as_str().unwrap();
        assert!(["owner-computer", "agent"].contains(&device), "{s}");
        assert!(
            ["would-do", "skipped", "pending"].contains(&s["status"].as_str().unwrap()),
            "{s}"
        );
    }
    assert!(steps.iter().any(|s| s["device"] == "agent"));
    assert!(steps.iter().any(|s| s["device"] == "owner-computer"));
    assert!(e.mutations().is_empty(), "{:?}", e.mutations());
    assert!(e.fake.state.lock().unwrap().manifest_attempts.is_empty());
    assert!(!git_ok(
        e.repo.path(),
        &["rev-parse", "--verify", "-q", "refs/heads/bosun/gate-app"]
    ));
    assert!(!e.repo.path().join(".bosun").exists());
    assert!(!git_ok(
        e.origin.path(),
        &["rev-parse", "--verify", "-q", "bosun/gate-app"]
    ));
}

#[test]
fn the_key_file_is_deleted_only_when_asked() {
    for (flag, gone) in [(false, false), (true, true)] {
        let e = Env::new();
        let dir = tempfile::tempdir().unwrap();
        let pem = dir.path().join("app.pem");
        std::fs::write(&pem, KEY).unwrap();
        std::fs::set_permissions(&pem, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        let mut args: Vec<&str> = MANUAL[..9].to_vec();
        args.extend(["--key-file", pem.to_str().unwrap()]);
        if flag {
            args.push("--delete-key-file");
        }
        let (out, v) = e.run(&args, "");
        assert!(out.status.success(), "{v}");
        assert_eq!(pem.exists(), !gone, "flag {flag}");
        assert_eq!(e.unseal(APP_KEY), KEY);
    }
}

#[test]
fn a_known_app_without_its_key_needs_the_owner_not_a_new_app() {
    let e = Env::new();
    let (out, _) = e.run(&MANUAL, KEY);
    assert!(out.status.success());
    // Someone removed the App key secret and the variable: the key cannot be recovered.
    {
        let mut s = e.fake.state.lock().unwrap();
        s.env_secrets.remove(APP_KEY);
        s.env_vars.remove(APP_VAR);
    }
    let (out, v) = e.run(&["owner", "setup"], "");
    assert!(!out.status.success(), "{v}");
    assert_eq!(out.status.code(), Some(5), "{v}");
    assert!(v["error"]["fix"].as_str().unwrap().contains("--manual"));
    assert!(e.fake.state.lock().unwrap().manifest_attempts.is_empty());
}

#[test]
fn the_secret_is_sealed_for_the_environment_key() {
    let secret = bosun::github::Secret::new("plain-value-canary".into());
    let pk = env_secret_key().public_key();
    let sealed =
        bosun::github::environments::seal(&bosun::github::b64::encode(pk.as_bytes()), &secret)
            .unwrap();
    assert!(!sealed.contains("plain-value-canary"));
    let raw = bosun::github::b64::decode(&sealed).unwrap();
    assert_eq!(
        env_secret_key().unseal(&raw).unwrap(),
        b"plain-value-canary"
    );
    let again =
        bosun::github::environments::seal(&bosun::github::b64::encode(pk.as_bytes()), &secret)
            .unwrap();
    assert_ne!(sealed, again, "a sealed box is randomised");
    assert!(bosun::github::environments::seal("not-a-key", &secret).is_err());
    assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
}

/// The browser flow end to end: read the start URL from stderr, play the browser by hand.
#[test]
fn uc_b2_1_with_the_manifest_flow() {
    let e = Env::new();
    let mut child = e
        .cmd()
        .args(["owner", "setup", "--timeout-secs", "30"])
        .spawn_std();
    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut port = None;
    let mut line = String::new();
    while port.is_none() {
        line.clear();
        assert!(err.read_line(&mut line).unwrap() > 0, "no start URL");
        port = line
            .split("http://127.0.0.1:")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .and_then(|p| p.parse::<u16>().ok());
    }
    let port = port.unwrap();
    let get = |target: &str| {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        write!(
            s,
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let page = get("/start");
    let state = page
        .split("?state=")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    get(&format!(
        "/callback?code=c0de1234abcd5678ef90&state={state}"
    ));
    let mut rest = String::new();
    err.read_to_string(&mut rest).unwrap();
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    let status = child.wait().unwrap();
    let v = one_object(stdout.as_bytes());
    assert!(status.success(), "{v} {rest}");
    assert_eq!(v["data"]["app_id"], 777);
    assert!(!rest.contains("KEYMATERIAL") && !stdout.contains("KEYMATERIAL"));
    assert_eq!(e.unseal(APP_KEY), KEY);
    assert_eq!(e.fake.state.lock().unwrap().manifest_attempts.len(), 1);
}

fn merge_enrollment_into_main_and_leave_a_stale_branch(e: &Env) {
    git(
        e.repo.path(),
        &["merge", "-q", "--ff-only", "bosun/gate-app"],
    );
    git(e.repo.path(), &["push", "-q", "origin", "main"]);
    git(e.repo.path(), &["branch", "-f", "bosun/gate-app", "HEAD~1"]);
    git(e.origin.path(), &["branch", "-D", "bosun/gate-app"]);
}

#[test]
fn a_stale_local_branch_never_replaces_a_working_signer_key() {
    let e = Env::new();
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    let before = e.sealed(SIGNER_KEY);
    merge_enrollment_into_main_and_leave_a_stale_branch(&e);
    let (out, v) = e.run(&["owner", "setup"], "");
    assert!(out.status.success(), "{v}");
    assert_eq!(e.sealed(SIGNER_KEY), before);
    assert!(v["data"].get("signer_public_key").is_none(), "{v}");
}

#[test]
fn a_bad_approvers_file_fails_before_the_signer_secret_is_set() {
    let e = Env::new();
    std::fs::create_dir_all(e.repo.path().join(".bosun")).unwrap();
    std::fs::write(e.repo.path().join(".bosun/approvers.toml"), "key = \"x\"\n").unwrap();
    git(e.repo.path(), &["add", "-A"]);
    git(e.repo.path(), &["commit", "-q", "-m", "bad approvers"]);
    git(e.repo.path(), &["push", "-q", "origin", "main"]);
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(!out.status.success(), "{v}");
    assert!(e.sealed(SIGNER_KEY).is_none());
}

#[test]
fn a_fresh_manual_app_replaces_a_stale_client_id_variable() {
    let e = Env::new();
    e.fake
        .state
        .lock()
        .unwrap()
        .env_vars
        .insert(APP_VAR.into(), "Iv1.stale".into());
    // The variable exists in an environment the run creates: make it exist first.
    e.fake.environment(
        "bosun-app",
        serde_json::json!({"protected_branches": false, "custom_branch_policies": true}),
        serde_json::json!([{"name": "main", "type": "branch"}]),
    );
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    assert_eq!(
        e.fake.state.lock().unwrap().env_vars[APP_VAR],
        "Iv1.fakeclient"
    );
}

#[test]
fn the_key_file_survives_when_the_app_secret_was_not_set_this_run() {
    let e = Env::new();
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    e.fake.state.lock().unwrap().env_vars.remove(APP_VAR);
    let dir = tempfile::tempdir().unwrap();
    let pem = dir.path().join("app.pem");
    std::fs::write(&pem, KEY).unwrap();
    std::fs::set_permissions(&pem, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    let sealed = e.sealed(APP_KEY);
    let mut args: Vec<&str> = MANUAL[..9].to_vec();
    args.extend(["--key-file", pem.to_str().unwrap(), "--delete-key-file"]);
    let (out, v) = e.run(&args, "");
    assert!(out.status.success(), "{v}");
    assert_eq!(e.sealed(APP_KEY), sealed, "the App secret is not touched");
    assert!(pem.exists(), "the key was not stored this run");
}

#[test]
fn a_remote_that_looks_like_an_option_is_refused() {
    let e = Env::new();
    let (out, v) = e.run(&["owner", "setup", "--remote=-oProxy"], "");
    assert_eq!(out.status.code(), Some(2), "{v}");
    assert!(e.mutations().is_empty());
}

#[test]
fn a_manifest_slug_with_toml_syntax_is_refused() {
    let e = Env::new();
    e.fake.state.lock().unwrap().manifest_slug = Some("x\"\nid = 1\nslug = \"y".into());
    let (child, stdout, rest) = play_manifest(&e);
    assert!(!child.success(), "{stdout} {rest}");
    assert!(!git_ok(
        e.repo.path(),
        &["rev-parse", "--verify", "-q", "refs/heads/bosun/gate-app"]
    ));
    assert!(e.sealed(APP_KEY).is_none());
}

#[test]
fn only_a_rejected_non_fast_forward_push_is_a_conflict() {
    let e = Env::new();
    let mut args = MANUAL.to_vec();
    args.extend(["--remote", "nowhere"]);
    let (out, v) = e.run(&args, KEY);
    assert_eq!(out.status.code(), Some(1), "{v}");

    let e = Env::new();
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    let tree = git(e.repo.path(), &["rev-parse", "bosun/gate-app^{tree}"]);
    let main = git(e.repo.path(), &["rev-parse", "main"]);
    let x = git(
        e.repo.path(),
        &["commit-tree", tree.trim(), "-p", main.trim(), "-m", "other"],
    );
    git(
        e.repo.path(),
        &["update-ref", "refs/heads/bosun/gate-app", x.trim()],
    );
    let (out, v) = e.run(&["owner", "setup"], "");
    assert_eq!(out.status.code(), Some(3), "{v}");
}

#[test]
fn a_main_only_environment_with_reviewers_is_fixed() {
    let e = Env::new();
    e.fake.environment(
        "bosun-app",
        serde_json::json!({"protected_branches": false, "custom_branch_policies": true}),
        serde_json::json!([{"name": "main", "type": "branch"}]),
    );
    e.fake
        .state
        .lock()
        .unwrap()
        .env_with_reviewers
        .push("bosun-app".into());
    let (out, v) = e.run(&MANUAL, KEY);
    assert!(out.status.success(), "{v}");
    assert!(e.fake.state.lock().unwrap().env_with_reviewers.is_empty());
    assert!(
        e.mutations()
            .iter()
            .any(|m| m == "PUT /repos/acme/widget/environments/bosun-app"),
        "{:?}",
        e.mutations()
    );
}

/// The browser flow with the browser played by hand: (exit status, stdout, stderr rest).
fn play_manifest(e: &Env) -> (std::process::ExitStatus, String, String) {
    let mut child = e
        .cmd()
        .args(["owner", "setup", "--timeout-secs", "30"])
        .spawn_std();
    let mut err = BufReader::new(child.stderr.take().unwrap());
    let mut port = None;
    let mut line = String::new();
    while port.is_none() {
        line.clear();
        assert!(err.read_line(&mut line).unwrap() > 0, "no start URL");
        port = line
            .split("http://127.0.0.1:")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .and_then(|p| p.parse::<u16>().ok());
    }
    let port = port.unwrap();
    let get = |target: &str| {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        write!(
            s,
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let page = get("/start");
    let state = page
        .split("?state=")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    get(&format!(
        "/callback?code=c0de1234abcd5678ef90&state={state}"
    ));
    let mut rest = String::new();
    err.read_to_string(&mut rest).unwrap();
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    (child.wait().unwrap(), stdout, rest)
}

trait SpawnStd {
    fn spawn_std(&mut self) -> std::process::Child;
}

impl SpawnStd for assert_cmd::Command {
    fn spawn_std(&mut self) -> std::process::Child {
        let mut c = std::process::Command::new(self.get_program());
        c.args(self.get_args());
        for (k, v) in self.get_envs() {
            match v {
                Some(v) => c.env(k, v),
                None => c.env_remove(k),
            };
        }
        if let Some(d) = self.get_current_dir() {
            c.current_dir(d);
        }
        c.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }
}
