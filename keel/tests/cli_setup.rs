//! `keel plan|up` for an existing repo (`template = "none"`, the root `keel.toml`) end to end:
//! stub `gh` and `hoist` on PATH, hoist's JSON from fixtures. Never a real tool but `git init`.
use assert_cmd::Command;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const GH: &str = r#"#!/bin/sh
echo "gh $*" >> "$KEEL_LOG/calls.log"
case "$*" in
  "auth status"*) exit 0;;
  "api user --jq .login") echo octo; exit 0;;
  "api repos/keelstackhq/keelstack --jq .permissions.admin") echo true; exit 0;;
esac
echo "unexpected gh call: $*" >&2
exit 1
"#;

const HOIST: &str = r#"#!/bin/sh
echo "hoist $*" >> "$KEEL_LOG/calls.log"
case "$*" in
  "--version") echo "hoist 0.21.0"; exit 0;;
  "--json explain"*) cat "$FIXTURES/explain.json"; exit 0;;
  "--json check --area"*) cat "$FIXTURES/check-no-cloudflare.json"; exit 2;;
  "--json check") cat "$FIXTURES/check.json"; exit 2;;
esac
echo "unexpected hoist call: $*" >&2
exit 3
"#;

struct Env {
    _root: tempfile::TempDir,
    stubs: PathBuf,
    log: PathBuf,
    repo: PathBuf,
    fixtures: PathBuf,
}

fn script(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(repo: &Path, args: &[&str]) {
    let st = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

impl Env {
    fn new() -> Env {
        let root = tempfile::tempdir().unwrap();
        let p = |n: &str| {
            let d = root.path().join(n);
            std::fs::create_dir(&d).unwrap();
            d
        };
        let (stubs, log, repo, fixtures) = (p("stubs"), p("log"), p("repo"), p("fixtures"));
        script(&stubs, "gh", GH);
        script(&stubs, "hoist", HOIST);
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let check = std::fs::read_to_string(src.join("keelstack-hoist-check.json")).unwrap();
        let explain = std::fs::read_to_string(src.join("keelstack-hoist-explain.json")).unwrap();
        // hoist with `--area` (no cloudflare) does not read the cloudflare area.
        let mut v: Value = serde_json::from_str(&check).unwrap();
        v["items"]
            .as_array_mut()
            .unwrap()
            .retain(|i| i["area"] != "cloudflare");
        std::fs::write(fixtures.join("check.json"), &check).unwrap();
        std::fs::write(fixtures.join("explain.json"), &explain).unwrap();
        std::fs::write(fixtures.join("check-no-cloudflare.json"), v.to_string()).unwrap();
        // The real root keel.toml.
        let toml = Path::new(env!("CARGO_MANIFEST_DIR")).join("../keel.toml");
        std::fs::copy(toml, repo.join("keel.toml")).unwrap();
        git(&repo, &["init", "-q"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/keelstackhq/keelstack",
            ],
        );
        Env {
            _root: root,
            stubs,
            log,
            repo,
            fixtures,
        }
    }

    fn keel(&self, cloudflare: bool, args: &[&str]) -> Command {
        let mut c = Command::cargo_bin("keel").unwrap();
        c.env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.stubs.display()))
            .env("HOME", &self.log)
            .env("TMPDIR", &self.log)
            .env("KEEL_LOG", &self.log)
            .env("FIXTURES", &self.fixtures)
            .arg("-C")
            .arg(&self.repo)
            .arg("--json")
            .args(args);
        if cloudflare {
            c.env("CLOUDFLARE_API_TOKEN", "x");
        }
        c
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.log.join("calls.log")).unwrap_or_default()
    }
}

fn run(c: &mut Command, code: i32) -> Value {
    let out = c.assert().code(code).get_output().stdout.clone();
    serde_json::from_slice(&out).expect("one JSON object")
}

/// Every item of .github/SETUP.md (generic strings: ids, names, packages).
fn setup_needles() -> Vec<String> {
    let mut n: Vec<String> = [
        "hoist: default branch",
        "ci-ok",
        "risk-check",
        "review-verdict",
        "main-open",
        "merge queue",
        "hoist: tags",
        "v*",
        "github-actions",
        "release",
        "bosun-app",
        "bosun-signer",
        "review",
        "hoard-r2",
        "site",
        "BOSUN_APP_CLIENT_ID",
        "BOSUN_REVIEW_CI",
        "BOSUN_APP_KEY",
        "BOSUN_SIGNER_KEY",
        "R2_ACCOUNT_ID",
        "R2_ACCESS_KEY_ID",
        "R2_SECRET_ACCESS_KEY",
        "R2_BUCKET",
        "ANTHROPIC_API_KEY",
        "keelstackhq-bosun-gate",
        "https://github.com/apps/keelstackhq-bosun-gate/installations/new",
        "hoist secrets set BOSUN_APP_KEY",
        "hoist release bootstrap",
        "keelstack-landing",
        "CLOUDFLARE_API_TOKEN",
        "CLOUDFLARE_ACCOUNT_ID",
        "@keelstackhq/cargo-deny",
        "@keelstackhq/cargo-deny-linux-x64",
        "@keelstackhq/cargo-deny-linux-arm64",
        "cargo-deny-npm.yml",
        "keel-bosun",
        "keel-hoist",
        "keel-berth",
        "keel-cli",
        "first-publish:keel-cli",
        "crates-trust:keel-bosun",
    ]
    .map(String::from)
    .to_vec();
    for t in ["bosun", "splice", "hoist", "berth", "keel"] {
        n.push(format!("@keelstackhq/{t}"));
        if t != "splice" {
            for p in [
                "linux-x64",
                "linux-arm64",
                "darwin-x64",
                "darwin-arm64",
                "win32-x64",
            ] {
                n.push(format!("@keelstackhq/{t}-{p}"));
            }
        }
    }
    n
}

fn no_writes(e: &Env) {
    let calls = e.calls();
    assert!(!calls.contains("apply"), "plan wrote: {calls}");
    assert!(calls.contains("hoist --json check"), "{calls}");
    assert!(
        calls.lines().all(|l| l.starts_with("hoist --json check")
            || l.starts_with("hoist --json explain")
            || l.starts_with("hoist --version")
            || l.starts_with("gh auth status")
            || l.starts_with("gh api user --jq")
            || l.starts_with("gh api repos/keelstackhq/keelstack --jq")),
        "{calls}"
    );
}

#[test]
fn plan_lists_every_setup_item_and_exits_5_without_writing() {
    let e = Env::new();
    let v = run(&mut e.keel(true, &["plan"]), 5);
    assert_eq!(
        (v["status"].as_str(), v["exit_code"].as_i64()),
        (Some("human"), Some(5))
    );
    assert_eq!(v["steps"][0]["id"], "preflight");
    assert_eq!(v["steps"][0]["action"], "ok");
    let text = format!("{} {}", v["items"], v["checklist"]);
    for n in setup_needles() {
        assert!(text.contains(&n), "missing from items/checklist: {n}");
    }
    assert!(v["items"][0]["step"] == "settings");
    assert_eq!(v["human_step"]["id"], v["checklist"][0]["id"]);
    let c = v["checklist"].as_array().unwrap();
    // Computer entries first, then phone; trust ids are phone entries that need --confirm.
    let devices: Vec<&str> = c.iter().map(|x| x["device"].as_str().unwrap()).collect();
    let first_phone = devices.iter().position(|d| *d == "phone").unwrap();
    assert!(devices[..first_phone].iter().all(|d| *d == "computer"));
    assert!(devices[first_phone..].iter().all(|d| *d == "phone"));
    let secret = c
        .iter()
        .find(|x| x["id"] == "secret:BOSUN_APP_KEY")
        .unwrap();
    assert_eq!(secret["command"], "hoist secrets set BOSUN_APP_KEY");
    assert_eq!(secret["verify"], "keel plan re-checks it");
    let trust = c
        .iter()
        .find(|x| x["id"] == "npm-trust:@keelstackhq/bosun")
        .unwrap();
    assert_eq!(trust["device"], "phone");
    assert_eq!(
        trust["verify"],
        "hoist cannot read it: run keel up --confirm npm-trust:@keelstackhq/bosun once done"
    );
    assert!(c.iter().all(|x| x["id"] != "cloudflare-token"));
    no_writes(&e);
    // plan never writes keel.toml either.
    let toml = std::fs::read_to_string(e.repo.join("keel.toml")).unwrap();
    assert!(!toml.contains("[confirmed]"), "{toml}");
    // The whole-area check: no --area flag.
    assert!(
        e.calls().lines().any(|l| l == "hoist --json check"),
        "{}",
        e.calls()
    );
}

#[test]
fn without_the_cloudflare_token_the_checklist_asks_for_it() {
    let e = Env::new();
    let v = run(&mut e.keel(false, &["plan"]), 5);
    let c = v["checklist"].as_array().unwrap();
    let t = c.iter().find(|x| x["id"] == "cloudflare-token").unwrap();
    assert_eq!(c[0]["id"], "cloudflare-token");
    assert_eq!(t["device"], "computer");
    let title = t["title"].as_str().unwrap();
    assert!(title.contains("CLOUDFLARE_API_TOKEN"), "{title}");
    assert!(title.contains("keelstack-landing"), "{title}");
    assert!(
        t["command"]
            .as_str()
            .unwrap()
            .contains("CLOUDFLARE_ACCOUNT_ID"),
        "{t}"
    );
    let calls = e.calls();
    assert!(
        calls.contains("hoist --json check --area github --area npm --area crates --area release"),
        "{calls}"
    );
    assert!(calls.contains("hoist --json explain"), "{calls}");
    no_writes(&e);
}

#[test]
fn confirm_removes_a_trust_step_and_is_written_to_keel_toml() {
    let e = Env::new();
    let id = "npm-trust:@keelstackhq/bosun";
    let before = run(&mut e.keel(true, &["plan"]), 5);
    assert!(
        before["checklist"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == id)
    );
    // `up --confirm` writes keel.toml before any step; hoist check exits 2 (a human step), so
    // hoist apply (an unexpected stub call) is never reached.
    let _ = e.keel(true, &["up", "--confirm", id]).assert();
    let toml = std::fs::read_to_string(e.repo.join("keel.toml")).unwrap();
    assert!(toml.contains("[confirmed]"), "{toml}");
    assert!(toml.contains(id), "{toml}");
    assert!(toml.contains("template = \"none\""), "{toml}");
    let after = run(&mut e.keel(true, &["plan"]), 5);
    let ids: Vec<&str> = after["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&id), "{ids:?}");
    assert!(ids.contains(&"npm-trust:@keelstackhq/hoist"));
    for plat in [
        "linux-x64",
        "linux-arm64",
        "darwin-x64",
        "darwin-arm64",
        "win32-x64",
    ] {
        let sp = format!("npm-trust:@keelstackhq/splice-{plat}");
        assert!(ids.contains(&sp.as_str()), "{sp}");
    }
    assert_eq!(ids.len(), before["checklist"].as_array().unwrap().len() - 1);
}

#[test]
fn pieces_with_template_none_is_a_manifest_error() {
    let e = Env::new();
    let mut t = std::fs::read_to_string(e.repo.join("keel.toml")).unwrap();
    t.push_str("pieces = [\"rust-cli:x\"]\n");
    std::fs::write(e.repo.join("keel.toml"), t).unwrap();
    let v = run(&mut e.keel(true, &["plan"]), 3);
    assert_eq!(v["status"], "error");
}

#[test]
fn text_output_prints_the_numbered_checklist() {
    let e = Env::new();
    let mut c = Command::cargo_bin("keel").unwrap();
    let out = c
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", e.stubs.display()))
        .env("HOME", &e.log)
        .env("KEEL_LOG", &e.log)
        .env("FIXTURES", &e.fixtures)
        .env("CLOUDFLARE_API_TOKEN", "x")
        .arg("-C")
        .arg(&e.repo)
        .arg("plan")
        .assert()
        .code(5)
        .get_output()
        .stdout
        .clone();
    let t = String::from_utf8(out).unwrap();
    assert!(t.contains("\nchecklist:\n  1. [computer] "), "{t}");
    assert!(
        t.contains("[phone] npm trust github @keelstackhq/bosun "),
        "{t}"
    );
}
