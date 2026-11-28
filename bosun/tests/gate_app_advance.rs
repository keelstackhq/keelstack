//! `bosun gate-app advance` (B3 T1): one step of the gate-App transition per run, state read from
//! the repo and GitHub, against the fake GitHub, a local bare remote and a stub splice.

mod common;

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::fake::FakeGithub;
use common::{bosun, one_object};
use serde_json::Value;

const SLUG: &str = "keelstackhq-bosun-gate";
const GATE_APP: &str = "id = 4242\nslug = \"keelstackhq-bosun-gate\"\n";
const CONFIG: &str =
    "project = \"widget\"\n\n[approvals]\npolicy = \"quorum\"\nowner_delegation = \"all\"\n";
const DUAL: &str = "project = \"widget\"\n\n[approvals]\npolicy = \"quorum\"\nowner_delegation = \"all\"\n\n[checks]\nmode = \"dual\"\napp_id = 4242\napp_slug = \"keelstackhq-bosun-gate\"\n";
const HOIST: &str =
    "[github]\nrequired_checks = [\"ci-ok\", \"risk-check\"]\nmerge_queue = false\n";

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

struct Env {
    fake: FakeGithub,
    repo: tempfile::TempDir,
    origin: tempfile::TempDir,
    home: tempfile::TempDir,
    bin: tempfile::TempDir,
}

impl Env {
    /// A repo on main with `.bosun/bosun.toml`, `hoist.toml` and the bosun channel lock; the
    /// gate-app record is on main only when `on_main`.
    fn new(on_main: bool, config: &str) -> Env {
        let fake = FakeGithub::start();
        let repo = tempfile::tempdir().unwrap();
        let origin = tempfile::tempdir().unwrap();
        git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(repo.path(), &["init", "-q", "-b", "main"]);
        let w = |p: &str, t: &str| {
            let f = repo.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, t).unwrap();
        };
        w(".bosun/bosun.toml", config);
        w("hoist.toml", HOIST);
        w(".splice/channels/bosun.toml", "x = 1\n");
        if on_main {
            w(".bosun/gate-app.toml", GATE_APP);
            fake.content(".bosun/gate-app.toml", "main", GATE_APP);
        }
        fake.content(".bosun/bosun.toml", "main", config);
        git(repo.path(), &["add", "-A"]);
        git(repo.path(), &["commit", "-q", "-m", "init"]);
        git(
            repo.path(),
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        git(repo.path(), &["push", "-q", "origin", "main"]);
        let bin = tempfile::tempdir().unwrap();
        let stub = bin.path().join("splice");
        let mut f = std::fs::File::create(&stub).unwrap();
        writeln!(
            f,
            "#!/bin/sh\necho \"$@\" >> \"$HOME/splice.args\"\necho '{{\"ok\":true,\"changes\":[],\"messages\":[]}}'"
        )
        .unwrap();
        drop(f);
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        Env {
            fake,
            repo,
            origin,
            home: tempfile::tempdir().unwrap(),
            bin,
        }
    }

    fn run(&self, extra: &[&str]) -> (std::process::Output, Value) {
        let out = bosun()
            .current_dir(self.repo.path())
            .env("BOSUN_GITHUB_API", self.fake.url())
            .env("HOME", self.home.path())
            .env("BOSUN_SPLICE", self.bin.path().join("splice"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(["gate-app", "advance"])
            .args(extra)
            .output()
            .unwrap();
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

    fn splice_calls(&self) -> String {
        std::fs::read_to_string(self.home.path().join("splice.args")).unwrap_or_default()
    }

    fn remote(&self, branch: &str, path: &str) -> String {
        git(self.origin.path(), &["show", &format!("{branch}:{path}")])
    }

    fn clean_on_main(&self) {
        assert_eq!(
            git(self.repo.path(), &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
            "main"
        );
        assert_eq!(git(self.repo.path(), &["status", "--porcelain"]), "");
    }

    /// Main now says PR A merged, and the App's runs are on an open probe PR.
    fn after_pr_a(&self) {
        self.fake.content(".bosun/bosun.toml", "main", DUAL);
        self.fake.content(".bosun/gate-app.toml", "main", GATE_APP);
    }

    fn probe_runs(&self) {
        self.fake.pull(9, "probe", "probe/x", "");
        self.fake
            .app_run("sha9", "risk-check", "success", "ok", SLUG, 4242);
    }
}

fn step_status<'a>(v: &'a Value, step: &str) -> &'a str {
    v["data"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == step)
        .unwrap_or_else(|| panic!("no step {step}: {v}"))["status"]
        .as_str()
        .unwrap()
}

#[test]
fn without_gate_app_toml_it_points_at_owner_setup() {
    let e = Env::new(false, CONFIG);
    let (out, v) = e.run(&[]);
    assert_eq!(out.status.code(), Some(5), "{v}");
    assert_eq!(v["data"]["step"], "owner-setup", "{v}");
    assert!(
        v["error"]["fix"]
            .as_str()
            .unwrap()
            .contains("bosun owner setup")
    );
    assert!(v["error"]["fix"].as_str().unwrap().contains("keel owner"));
    assert!(e.mutations().is_empty());
}

#[test]
fn pr_a_is_made_on_a_branch_and_never_merged() {
    let e = Env::new(true, CONFIG);
    let (out, v) = e.run(&[]);
    assert!(out.status.success(), "{v}");
    assert_eq!(v["data"]["step"], "pr-a");
    assert_eq!(v["data"]["branch"], "bosun/gate-app");
    let n = v["data"]["pr"]["number"].as_u64().unwrap();
    assert!(e.fake.pull_is_draft(n));
    assert!(e.fake.auto_merge_enabled(n).is_none());
    assert!(!e.fake.merged(n));
    assert!(
        e.mutations().iter().all(|m| !m.contains("/merge")),
        "{:?}",
        e.mutations()
    );
    let cfg = e.remote("bosun/gate-app", ".bosun/bosun.toml");
    let t: toml::Table = cfg.parse().unwrap();
    assert_eq!(t["checks"]["mode"].as_str(), Some("dual"));
    assert_eq!(t["checks"]["app_id"].as_integer(), Some(4242));
    assert_eq!(t["checks"]["app_slug"].as_str(), Some(SLUG));
    assert_eq!(t["approvals"]["owner_delegation"].as_str(), Some("all"));
    let hoist = e.remote("bosun/gate-app", "hoist.toml");
    let h: toml::Table = hoist.parse().unwrap();
    let rc = h["github"]["required_checks"].as_array().unwrap();
    let find = |c: &str| {
        rc.iter()
            .find(|x| x["context"].as_str() == Some(c))
            .unwrap()
    };
    assert_eq!(find("ci-ok")["integration_id"].as_integer(), Some(15368));
    for g in ["risk-check", "review-verdict", "main-open"] {
        assert_eq!(find(g)["app"].as_str(), Some(SLUG), "{hoist}");
    }
    assert_eq!(h["github"]["merge_queue"].as_bool(), Some(false));
    assert!(
        e.splice_calls().contains("set --channel bosun bosun_app=1"),
        "{}",
        e.splice_calls()
    );
    e.clean_on_main();
    // The next run only reports: the PR is open.
    let calls = e.mutations().len();
    let (_, v2) = e.run(&[]);
    assert_eq!(v2["data"]["step"], "pr-a");
    assert_eq!(v2["data"]["status"], "waiting");
    assert_eq!(e.mutations().len(), calls, "{:?}", e.mutations());
}

#[test]
fn pr_a_reuses_the_owner_setup_branch_commit() {
    let e = Env::new(false, CONFIG);
    // What `owner setup` pushed: a commit on bosun/gate-app that records the App.
    git(e.repo.path(), &["switch", "-q", "-c", "bosun/gate-app"]);
    std::fs::create_dir_all(e.repo.path().join(".bosun")).unwrap();
    std::fs::write(e.repo.path().join(".bosun/gate-app.toml"), GATE_APP).unwrap();
    git(e.repo.path(), &["add", "-A"]);
    git(
        e.repo.path(),
        &["commit", "-q", "-m", "record the gate App"],
    );
    git(e.repo.path(), &["push", "-q", "origin", "bosun/gate-app"]);
    git(e.repo.path(), &["switch", "-q", "main"]);
    git(e.repo.path(), &["branch", "-q", "-D", "bosun/gate-app"]);
    let setup_tip = git(e.origin.path(), &["rev-parse", "bosun/gate-app"]);
    let (out, v) = e.run(&[]);
    assert!(out.status.success(), "{v}");
    assert_eq!(v["data"]["step"], "pr-a");
    let log = git(e.origin.path(), &["log", "--format=%H", "bosun/gate-app"]);
    assert!(
        log.contains(setup_tip.trim()),
        "the owner-setup commit is in PR A"
    );
    assert_eq!(e.remote("bosun/gate-app", ".bosun/gate-app.toml"), GATE_APP);
    assert!(
        e.remote("bosun/gate-app", ".bosun/bosun.toml")
            .contains("dual")
    );
    e.clean_on_main();
}

#[test]
fn dry_run_writes_nothing() {
    let e = Env::new(true, CONFIG);
    let (out, v) = e.run(&["--dry-run"]);
    assert!(out.status.success(), "{v}");
    assert_eq!(v["data"]["dry_run"], true);
    assert_eq!(v["data"]["step"], "pr-a");
    assert_eq!(step_status(&v, "pr-a"), "would-do");
    assert!(e.mutations().is_empty(), "{:?}", e.mutations());
    assert_eq!(e.splice_calls(), "");
    assert!(!git(e.origin.path(), &["branch", "--list", "bosun/gate-app"]).contains("gate-app"));
    e.clean_on_main();
}

#[test]
fn after_pr_a_without_app_runs_it_waits_for_the_probe() {
    let e = Env::new(true, CONFIG);
    e.after_pr_a();
    e.fake.pull(9, "probe", "probe/x", "");
    let (out, v) = e.run(&[]);
    assert!(out.status.success(), "{v}");
    assert_eq!(v["data"]["step"], "probe");
    assert_eq!(v["data"]["status"], "waiting");
    assert_eq!(step_status(&v, "pr-a"), "done");
    assert!(e.mutations().is_empty(), "{:?}", e.mutations());
}

#[test]
fn app_runs_seen_and_ruleset_unpinned_names_the_owner_hoist_apply() {
    let e = Env::new(true, CONFIG);
    e.after_pr_a();
    e.probe_runs();
    e.fake.unpin_gates();
    let (out, v) = e.run(&[]);
    assert!(out.status.success(), "{v}");
    assert_eq!(v["data"]["step"], "hoist-apply");
    assert_eq!(step_status(&v, "probe"), "done");
    let next = v["next"].to_string();
    assert!(next.contains("hoist apply --area github"), "{next}");
    assert!(e.mutations().is_empty(), "{:?}", e.mutations());
}

#[test]
fn a_ruleset_pinned_to_another_app_is_not_pinned() {
    let e = Env::new(true, CONFIG);
    e.after_pr_a();
    e.probe_runs();
    e.fake
        .enforce_checks(&["risk-check@777", "review-verdict@4242", "main-open@4242"]);
    let (_, v) = e.run(&[]);
    assert_eq!(v["data"]["step"], "hoist-apply", "{v}");
}

#[test]
fn pinned_ruleset_opens_pr_b_with_checks_mode_and_the_signer() {
    let e = Env::new(true, CONFIG);
    e.after_pr_a();
    // main as the fake serves it, and as the checkout has it after PR A merged.
    std::fs::write(e.repo.path().join(".bosun/bosun.toml"), DUAL).unwrap();
    git(e.repo.path(), &["commit", "-qam", "PR A"]);
    git(e.repo.path(), &["push", "-q", "origin", "main"]);
    e.probe_runs();
    let (out, v) = e.run(&[]);
    assert!(out.status.success(), "{v}");
    assert_eq!(v["data"]["step"], "pr-b");
    assert_eq!(v["data"]["branch"], "bosun/gate-app-checks");
    let n = v["data"]["pr"]["number"].as_u64().unwrap();
    assert!(!e.fake.merged(n));
    let cfg = e.remote("bosun/gate-app-checks", ".bosun/bosun.toml");
    let t: toml::Table = cfg.parse().unwrap();
    assert_eq!(t["checks"]["mode"].as_str(), Some("checks"));
    assert_eq!(t["checks"]["app_id"].as_integer(), Some(4242));
    assert_eq!(t["approvals"]["signer"].as_bool(), Some(true));
    assert_eq!(t["approvals"]["owner_delegation"].as_str(), Some("all"));
    let left = v["data"]["left"].to_string();
    assert!(left.contains("signer"), "{left}");
    e.clean_on_main();
}

#[test]
fn everything_merged_is_done() {
    let checks = DUAL.replace("dual", "checks").replace(
        "owner_delegation = \"all\"\n",
        "owner_delegation = \"all\"\nsigner = true\n",
    );
    let e = Env::new(true, &checks);
    let (out, v) = e.run(&[]);
    assert!(out.status.success(), "{v}");
    assert_eq!(v["data"]["step"], "done");
    assert_eq!(step_status(&v, "pr-b"), "done");
    assert!(e.mutations().is_empty());
}
