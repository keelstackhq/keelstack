//! `bosun migrate --to autonomous` writes the D-028 flow set into `.bosun/bosun.toml`.
mod common;

use std::path::Path;
use std::process::Command;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::Value;

const BOSUN_TOML: &str = ".bosun/bosun.toml";

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

struct Env {
    work: tempfile::TempDir,
    _origin: tempfile::TempDir,
}

fn env(toml: &str) -> Env {
    let work = tempfile::tempdir().unwrap();
    let origin = tempfile::tempdir().unwrap();
    git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    git(work.path(), &["init", "-q", "-b", "main"]);
    for (rel, text) in [(BOSUN_TOML, toml), (".bosun/version", "0.14.0\n")] {
        let p = work.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    git(work.path(), &["add", "-A"]);
    git(work.path(), &["commit", "-q", "-m", "x"]);
    let url = origin.path().to_str().unwrap().to_string();
    git(work.path(), &["remote", "add", "origin", &url]);
    git(work.path(), &["push", "-q", "origin", "main"]);
    Env {
        work,
        _origin: origin,
    }
}

fn migrate(e: &Env, args: &[&str]) -> (i32, Value) {
    let fake = FakeGithub::start();
    let dry = args.contains(&"--dry-run");
    let out = bosun()
        .env("BOSUN_GITHUB_API", fake.url())
        .env("BOSUN_MIGRATE_METRICS_TIMEOUT", "0")
        .env_remove("CLAUDE_CODE_REMOTE_ENVIRONMENT_ID")
        .arg("-C")
        .arg(e.work.path())
        .arg("migrate")
        .args(args)
        .args((!dry).then_some("--no-ready"))
        .output()
        .unwrap();
    (out.status.code().unwrap(), one_object(&out.stdout))
}

fn toml(e: &Env) -> String {
    std::fs::read_to_string(e.work.path().join(BOSUN_TOML)).unwrap()
}

fn parsed(e: &Env) -> toml::Value {
    toml::from_str(&toml(e)).unwrap()
}

#[test]
fn autonomous_direct_writes_the_d028_set_and_keeps_the_rest() {
    let e = env(
        "# the widget\nproject = \"widget\"\n\n[approvals]\n# who signs\npolicy = \"owner\"\nbootstrap_until = \"2026-01-01\"\n\n[review]\nmodel = \"m\"\n",
    );
    let (code, v) = migrate(&e, &["--to", "autonomous", "--direct"]);
    assert_eq!(code, 0, "{v}");
    let t = toml(&e);
    assert!(t.starts_with("# the widget\nproject = \"widget\"\n"), "{t}");
    assert!(t.contains("# who signs\n"), "{t}");
    let p = parsed(&e);
    assert_eq!(p["mode"]["name"].as_str(), Some("autonomous"));
    assert_eq!(p["approvals"]["policy"].as_str(), Some("quorum"));
    assert_eq!(p["approvals"]["owner_delegation"].as_str(), Some("all"));
    assert_eq!(
        p["approvals"]["bootstrap_until"].as_str(),
        Some("2026-01-01")
    );
    assert_eq!(p["review"]["verdicts_needed"].as_integer(), Some(1));
    assert_eq!(p["review"]["block_on"].as_str(), Some("blocker"));
    assert_eq!(p["review"]["model"].as_str(), Some("m"));
    assert_eq!(p["main_health"]["block"].as_bool(), Some(false));
    assert!(p.get("cloud").is_none(), "{t}");
    // The file still loads as bosun config.
    let (code, _) = migrate(&e, &["--to", "autonomous", "--dry-run"]);
    assert_eq!(code, 0);
}

#[test]
fn cloud_env_is_written_only_when_given() {
    let e = env("project = \"widget\"\n");
    let (code, v) = migrate(
        &e,
        &["--to", "autonomous", "--direct", "--cloud-env", "env_01ABC"],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        parsed(&e)["cloud"]["environment_id"].as_str(),
        Some("env_01ABC")
    );
}

#[test]
fn the_preset_is_idempotent() {
    let done = "project = \"widget\"\n\n[mode]\nname = \"autonomous\"\n\n[approvals]\npolicy = \"quorum\"\nowner_delegation = \"all\"\n\n[review]\nverdicts_needed = 1\nblock_on = \"blocker\"\n\n[main_health]\nblock = false\n";
    let e = env(done);
    let (code, v) = migrate(&e, &["--to", "autonomous", "--direct", "--dry-run"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["data"]["changed"], false, "{v}");
    assert_eq!(v["data"]["files"], serde_json::json!([]));
    // A second application on the written text changes nothing.
    let e2 = env("project = \"widget\"\n");
    migrate(&e2, &["--to", "autonomous", "--direct"]);
    let once = toml(&e2);
    let e3 = env(&once);
    let (_, v) = migrate(&e3, &["--to", "autonomous", "--direct", "--dry-run"]);
    assert_eq!(v["data"]["changed"], false, "{v}");
}

#[test]
fn flow_gatekeeper_and_other_targets_get_no_preset() {
    let e = env("project = \"widget\"\n");
    let (code, v) = migrate(
        &e,
        &["--to", "autonomous", "--direct", "--flow", "gatekeeper"],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        toml(&e),
        "project = \"widget\"\n\n[mode]\nname = \"autonomous\"\n"
    );
    let e = env("project = \"widget\"\n");
    let (code, v) = migrate(
        &e,
        &["--to", "booster", "--direct", "--cloud-env", "env_01ABC"],
    );
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        toml(&e),
        "project = \"widget\"\n\n[mode]\nname = \"booster\"\n"
    );
}
