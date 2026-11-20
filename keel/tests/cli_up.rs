//! `keel up|plan` end to end against stub tools: shell scripts in a temp dir that is the only
//! non-system entry of PATH. Every tool keel can call is stubbed, so no real `gh`, `git`, `npm`,
//! `node` or `cargo` from /usr/bin is ever reached.
use assert_cmd::Command;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;

const STUB: &str = r#"#!/bin/sh
name=$(basename "$0")
echo "$name $*" >> "$KEEL_LOG/calls.log"
case "$name $*" in
  "gh secret set"*) cat >> "$KEEL_LOG/stdin.log"; : > "$KEEL_LOG/secret_set"; exit 0;;
esac
env() { printf '{"version":1,"command":"x","ok":true,"exit_code":0,"data":%s,"error":null,"next":[]}\n' "$1"; }
case "$name $*" in
  "node --version") echo v24.1.0;;
  "git --version") echo "git version 2.47.0";;
  "git rev-parse --is-inside-work-tree") echo true;;
  "git remote get-url origin") echo https://github.com/owner/name.git;;
  "git rev-parse --abbrev-ref HEAD") echo keel/bootstrap;;
  "gh auth status"*) ;;
  "gh api user --jq .login") echo octo;;
  "gh api user/installations --paginate"*)
    echo '{"id":1,"app_slug":"claude","repository_selection":"all"}'
    [ "$KEEL_SCENARIO" = fresh ] || echo '{"id":2,"app_slug":"renovate","repository_selection":"all"}';;
  "gh api repos/owner/name/environments/"*)
    [ "$KEEL_SCENARIO" = done ] || [ -f "$KEEL_LOG/secret_set" ] || exit 1;;
  "gh repo view"*) echo '{"defaultBranchRef":{"name":"main"}}';;
  "gh pr view"*)
    st=OPEN; [ "$KEEL_SCENARIO" = done ] && st=MERGED
    echo '{"number":7,"url":"https://github.com/owner/name/pull/7","state":"'$st'","headRefOid":"abc123"}';;
  "git ls-remote"*) printf '3f4a65fda568cdd20c76168ed156d25a28ba5630\trefs/tags/v0.13.0\n';;
  "splice --version") echo "splice 0.6.0";;
  "splice --json pieces"*) echo "error: unrecognized subcommand 'pieces'" >&2; exit 2;;
  "splice --json --yes new"*) echo '{"ok":true}';;
  "bosun --version") echo "bosun 0.21.0";;
  "bosun --json init"*) env '{"labels":[]}';;
  "bosun --json plan import"*) env '{"epics":[],"tasks":[]}';;
  "bosun --json setup-snippet") env '{"script":"echo SNIPPET"}';;
  "bosun --json doctor"*)
    if [ "$KEEL_SCENARIO" = offline ]; then
      printf '{"ok":false,"exit_code":5,"data":{"offline":true,"missing":0},"error":null}\n'; exit 5
    fi
    env '{"items":[],"missing":0}';;
  "hoist --version") echo "hoist 0.5.1";;
  "hoist --json "*) echo '{"ok":true,"exit_code":0,"items":[],"human_steps":[]}';;
  "berth --version") echo "berth 0.2.1";;
esac
exit 0
"#;

const TOOLS: [&str; 9] = [
    "gh", "git", "node", "npm", "splice", "bosun", "hoist", "berth", "cargo",
];

struct Env {
    _root: tempfile::TempDir,
    stubs: PathBuf,
    log: PathBuf,
    repo: PathBuf,
    home: PathBuf,
    pins: PathBuf,
}

impl Env {
    fn new() -> Env {
        let root = tempfile::tempdir().unwrap();
        let p = |n: &str| {
            let d = root.path().join(n);
            std::fs::create_dir(&d).unwrap();
            d
        };
        let (stubs, log, repo, home) = (p("stubs"), p("log"), p("repo"), p("home"));
        let script = stubs.join("stub.sh");
        std::fs::write(&script, STUB).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        for t in TOOLS {
            symlink(&script, stubs.join(t)).unwrap();
        }
        let pins = root.path().join("pins.toml");
        std::fs::write(
            &pins,
            "[pins]\nsplice = \"0.6.0\"\nbosun = \"0.21.0\"\nhoist = \"0.5.1\"\nberth = \"0.2.1\"\nhull = \"v0.13.0\"\n",
        )
        .unwrap();
        Env {
            _root: root,
            stubs,
            log,
            repo,
            home,
            pins,
        }
    }

    fn rendered(&self) {
        for (d, f) in [(".splice", "lock.toml"), (".bosun", "version")] {
            std::fs::create_dir(self.repo.join(d)).unwrap();
            std::fs::write(self.repo.join(d).join(f), "").unwrap();
        }
    }

    fn keel(&self, scenario: &str, args: &[&str]) -> Command {
        let mut c = Command::cargo_bin("keel").unwrap();
        c.env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.stubs.display()))
            .env("HOME", &self.home)
            .env("TMPDIR", &self.home)
            .env("KEEL_LOG", &self.log)
            .env("KEEL_SCENARIO", scenario)
            .env("KEEL_PINS", &self.pins)
            .env("KEEL_POLL_SECS", "1")
            .arg("-C")
            .arg(&self.repo)
            .args(args);
        c
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.log.join(name)).unwrap_or_default()
    }
}

const FLAGS: [&str; 7] = [
    "--repo",
    "owner/name",
    "--template",
    "rust",
    "--pieces",
    "rust-cli:my-cli",
    "--wait=0",
];

fn up(sub: &str, extra: &[&str]) -> Vec<String> {
    let mut v = vec!["--json".to_string(), sub.to_string()];
    v.extend(FLAGS.iter().map(|s| s.to_string()));
    v.extend(extra.iter().map(|s| s.to_string()));
    v
}

fn run(env: &Env, scenario: &str, args: &[String], stdin: &str) -> (i32, String, String) {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let o = env.keel(scenario, &a).write_stdin(stdin).output().unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

fn one_json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("one JSON object: {e}: {stdout}"))
}

#[test]
fn first_run_stops_at_apps_with_one_json_object() {
    let env = Env::new();
    let (code, out, _) = run(&env, "fresh", &up("up", &[]), "");
    assert_eq!(code, 5, "{out}");
    let v = one_json(&out);
    assert_eq!(v["status"], "human");
    assert_eq!(v["human_step"]["id"], "apps");
    let steps = v["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 9);
    assert_eq!(steps[0]["action"], "ok");
    assert_eq!(steps[1]["action"], "pending");
    assert_eq!(steps[2]["action"], "not-reached");
    // keel.toml was written first.
    let m = std::fs::read_to_string(env.repo.join("keel.toml")).unwrap();
    assert!(m.contains("repo = \"owner/name\""), "{m}");
    assert!(m.contains("template_ref = \"v0.13.0\""), "{m}");
    // keel.toml is written through a temp file that is renamed away.
    let names: Vec<String> = std::fs::read_dir(&env.repo)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(!names.iter().any(|n| n.ends_with(".tmp")), "{names:?}");
}

#[test]
fn invalid_repo_is_a_usage_error_exit_2_and_writes_nothing() {
    for bad in ["owner", "../x/y", "o/n;x", "o/.."] {
        let env = Env::new();
        let (code, out, _) = run(
            &env,
            "fresh",
            &["--json".into(), "up".into(), "--repo".into(), bad.into()],
            "",
        );
        assert_eq!(code, 2, "{bad}: {out}");
        let v = one_json(&out);
        assert_eq!(v["exit_code"], 2);
        assert_eq!(v["status"], "error");
        assert!(!env.repo.join("keel.toml").exists());
    }
}

#[test]
fn everything_done_exits_zero() {
    let env = Env::new();
    env.rendered();
    std::fs::write(
        env.repo.join("keel.toml"),
        "repo = \"owner/name\"\ntemplate = \"rust\"\npieces = [\"rust-cli:my-cli\"]\n[confirmed]\nagent_env = true\n",
    )
    .unwrap();
    let (code, out, err) = run(&env, "done", &["--json".into(), "up".into()], "");
    assert_eq!(code, 0, "{out} {err}");
    let v = one_json(&out);
    assert_eq!(v["status"], "done");
    assert_eq!(v["ok"], true);
    assert!(
        v["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["action"] == "ok")
    );
}

#[test]
fn plan_never_writes() {
    let env = Env::new();
    let (code, out, _) = run(&env, "plan", &up("plan", &[]), "");
    let v = one_json(&out);
    assert_eq!(v["command"], "plan");
    assert_eq!(code, 5, "{out}"); // stops at the token gate
    assert!(!env.repo.join("keel.toml").exists());
    assert!(!env.repo.join(".keel").exists());
    let calls = env.read("calls.log");
    assert!(calls.contains("gh auth status"), "{calls}");
    for write in [
        "gh secret set",
        "gh pr create",
        "gh label create",
        "git push",
        "git add",
        "git commit",
        "git checkout",
        "npm install",
        "npm pack",
        "splice --json --yes new",
        "bosun --json init",
        "bosun --json setup-snippet",
        "hoist --json apply",
        "cargo generate-lockfile",
    ] {
        assert!(!calls.contains(write), "plan ran `{write}`:\n{calls}");
    }
    for line in calls.lines().filter(|l| l.contains("plan import")) {
        assert!(line.contains("--dry-run"), "{line}");
    }
}

#[test]
fn missing_repo_is_a_usage_error_with_fix() {
    let env = Env::new();
    let (code, out, _) = run(&env, "plan", &["--json".into(), "up".into()], "");
    assert_eq!(code, 2);
    let v = one_json(&out);
    assert_eq!(v["status"], "error");
    assert!(
        v["error"]["fix"]
            .as_str()
            .unwrap()
            .starts_with("keel up --repo owner/name")
    );
    assert!(!env.repo.join("keel.toml").exists());
}

#[test]
fn missing_template_is_a_usage_error_with_fix() {
    let env = Env::new();
    let args = ["--json", "up", "--repo", "owner/name"].map(String::from);
    let (code, out, _) = run(&env, "plan", &args, "");
    assert_eq!(code, 2);
    let v = one_json(&out);
    assert!(v["error"]["fix"].as_str().unwrap().contains("--template"));
    assert!(!env.repo.join("keel.toml").exists());
}

#[test]
fn token_goes_to_gh_stdin_only() {
    let env = Env::new();
    env.rendered();
    let secret = "ghp_SuperSecretValue123";
    let (code, out, err) = run(
        &env,
        "token",
        &up("up", &["--token-stdin"]),
        &format!("{secret}\n"),
    );
    assert_eq!(code, 5, "{out} {err}"); // then the agent-env gate
    let v = one_json(&out);
    let calls = env.read("calls.log");
    assert!(
        !calls.contains("add -A") && !calls.contains("commit -a"),
        "{calls}"
    );
    assert_eq!(v["human_step"]["id"], "agent-env");
    assert_eq!(v["steps"][5]["id"], "token");
    assert_eq!(v["steps"][5]["action"], "done");
    assert!(env.read("stdin.log").contains(secret));
    assert!(
        env.read("calls.log")
            .contains("gh secret set SPLICE_BOT_TOKEN --env splice-bot --repo owner/name")
    );
    for (what, text) in [
        ("argv log", env.read("calls.log")),
        ("stdout", out),
        ("stderr", err),
        (
            "keel.toml",
            std::fs::read_to_string(env.repo.join("keel.toml")).unwrap(),
        ),
    ] {
        assert!(!text.contains(secret), "token leaked into {what}");
    }
    let sh = std::fs::read_to_string(env.repo.join(".keel/cloud-setup.sh")).unwrap();
    assert!(sh.contains("echo SNIPPET") && !sh.contains(secret));
}

#[test]
fn doctor_offline_is_a_human_step_with_exit_5() {
    let env = Env::new();
    env.rendered();
    std::fs::write(
        env.repo.join("keel.toml"),
        "repo = \"owner/name\"\ntemplate = \"rust\"\npieces = [\"rust-cli:my-cli\"]\n",
    )
    .unwrap();
    let (code, out, err) = run(&env, "offline", &["--json".into(), "doctor".into()], "");
    assert_eq!(code, 5, "{out} {err}");
    let v = one_json(&out);
    assert_eq!(v["command"], "doctor");
    assert_eq!(v["status"], "human");
    assert_eq!(
        v["human_step"]["command"],
        "export GH_TOKEN=$(gh auth token)"
    );
}
