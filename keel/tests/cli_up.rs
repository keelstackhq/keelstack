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
  "gh api repos/owner/name/environments/production/secrets/"*)
    [ "$KEEL_SCENARIO" = done ] || [ -f "$KEEL_LOG/fly_setup" ] || exit 1;;
  "gh api repos/owner/name/environments/"*)
    [ "$KEEL_SCENARIO" = deploy ] && exit 0;
    [ "$KEEL_SCENARIO" = done ] || [ -f "$KEEL_LOG/secret_set" ] || exit 1;;
  "gh repo view"*) echo '{"defaultBranchRef":{"name":"main"}}';;
  "gh pr view"*)
    st=OPEN; { [ "$KEEL_SCENARIO" = done ] || [ -f "$KEEL_LOG/merged" ]; } && st=MERGED
    echo '{"number":7,"url":"https://github.com/owner/name/pull/7","state":"'$st'","headRefOid":"abc123","mergeCommit":{"oid":"c0ffee1"}}';;
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
  "fly auth whoami"*) echo '{"email":"a@b.c"}';;
  "fly status"*|"fly certs show"*)
    [ "$KEEL_SCENARIO" = done ] || [ -f "$KEEL_LOG/fly_setup" ] || exit 1
    echo '{}';;
  "fly ips list"*) echo '[{"Address":"1.2.3.4","Type":"v4"},{"Address":"2a09::1","Type":"v6"}]';;
  "gh run list"*) echo '[{"status":"completed","conclusion":"success","url":"https://github.com/owner/name/actions/runs/1","headSha":"c0ffee1"}]';;
  curl*) printf 200;;
  "hoist --version") echo "hoist 0.5.1";;
  "hoist --json check --area cloudflare"*) echo '{"ok":true,"exit_code":0,"items":[{"kind":"dns_record","key":"k","action":"ok"},{"kind":"r2_bucket","key":"b","action":"ok"},{"kind":"r2_lifecycle","key":"l","action":"ok"},{"kind":"r2_token","key":"t","action":"ok"}],"human_steps":[]}';;
  "hoist --json "*) echo '{"ok":true,"exit_code":0,"items":[],"human_steps":[]}';;
  "berth --version") echo "berth 0.2.1";;
esac
exit 0
"#;

const TOOLS: [&str; 11] = [
    "gh", "git", "node", "npm", "splice", "bosun", "hoist", "berth", "cargo", "fly", "curl",
];

/// The hull fly piece's script, as a stub: records its argv and that it ran.
const FLY_SETUP: &str =
    "#!/bin/sh\necho \"fly-setup.sh $*\" >> \"$KEEL_LOG/calls.log\"\n: > \"$KEEL_LOG/fly_setup\"\n";

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
            "[pins]\nsplice = \"0.6.0\"\nbosun = \"0.21.0\"\nhoist = \"0.5.1\"\nberth = \"0.2.1\"\nhull = \"v0.13.0\"\n[refs.hull]\ntag = \"v0.13.0\"\nsha = \"3f4a65fda568cdd20c76168ed156d25a28ba5630\"\n",
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
    assert_eq!(steps.len(), 13);
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

const FLY_MANIFEST: &str = "repo = \"owner/name\"\ntemplate = \"rust\"\npieces = [\"rust-cli:my-cli\", \"fly\"]\n[confirmed]\nagent_env = true\n[deploy.fly]\napp = \"my-app\"\nregion = \"fra\"\nhost = \"app.example.com\"\n";

fn fly_repo(env: &Env) {
    env.rendered();
    std::fs::write(env.repo.join("keel.toml"), FLY_MANIFEST).unwrap();
    std::fs::create_dir(env.repo.join("scripts")).unwrap();
    let s = env.repo.join("scripts/fly-setup.sh");
    std::fs::write(&s, FLY_SETUP).unwrap();
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn deploy_fly_reaches_the_merge_gate() {
    let env = Env::new();
    fly_repo(&env);
    let (code, out, err) = run(&env, "deploy", &["--json".into(), "up".into()], "");
    assert_eq!(code, 5, "{out} {err}");
    let v = one_json(&out);
    assert_eq!(v["human_step"]["id"], "merge");
    let by_id = |id: &str| {
        v["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(by_id("deploy")["action"], "done");
    assert_eq!(by_id("dns")["action"], "ok");
    assert!(
        by_id("dns")["detail"]
            .as_str()
            .unwrap()
            .contains("A app.example.com 1.2.3.4")
    );
    assert_eq!(by_id("backup")["detail"], "no [backup]");
    assert_eq!(by_id("verify")["action"], "not-reached");
    assert_eq!(v["verify"]["healthz"], "not-run");
    assert_eq!(v["verify"]["http_status"], serde_json::Value::Null);
    let calls = env.read("calls.log");
    assert!(
        calls.contains("fly-setup.sh --app my-app --region fra --host app.example.com"),
        "{calls}"
    );
    assert!(
        calls.contains("fly-setup.sh --app my-app --region fra --host app.example.com --set-gh-secret production"),
        "{calls}"
    );
    assert!(!calls.contains("--deploy"), "{calls}");
}

#[test]
fn merged_deploy_is_verified_over_http() {
    let env = Env::new();
    fly_repo(&env);
    let (code, out, err) = run(&env, "done", &["--json".into(), "up".into()], "");
    assert_eq!(code, 0, "{out} {err}");
    let v = one_json(&out);
    assert_eq!(v["verify"]["url"], "https://app.example.com");
    assert_eq!(v["verify"]["healthz"], "ok");
    assert_eq!(v["verify"]["http_status"], 200);
    assert_eq!(v["verify"]["smoke"], "none");
    let calls = env.read("calls.log");
    assert!(
        calls.contains(
            "curl -fsS -o /dev/null -w %{http_code} --max-time 10 https://app.example.com/healthz"
        ),
        "{calls}"
    );
    assert!(!calls.contains("fly-setup.sh"), "{calls}");
    // Without [deploy.fly] the report has no verify key.
    let env2 = Env::new();
    env2.rendered();
    std::fs::write(
        env2.repo.join("keel.toml"),
        FLY_MANIFEST.split("[deploy.fly]").next().unwrap(),
    )
    .unwrap();
    let (_, out, _) = run(&env2, "done", &["--json".into(), "up".into()], "");
    assert!(one_json(&out).get("verify").is_none());
}

/// The PAT and the Cloudflare token reach no argv log, stdout, stderr, hoist.toml, keel.toml or
/// `.keel/`, through deploy, dns, backup, merge and verify (the PR is already merged).
#[test]
fn pat_and_cloudflare_token_never_leak_through_deploy_dns_backup_merge_verify() {
    let env = Env::new();
    fly_repo(&env);
    std::fs::write(env.log.join("merged"), "").unwrap();
    let manifest = format!(
        "{FLY_MANIFEST}dns = \"cloudflare\"\n[backup]\nprovider = \"r2\"\naccount_id = \"0123456789abcdef0123456789abcdef\"\n"
    );
    std::fs::write(env.repo.join("keel.toml"), manifest).unwrap();
    let (pat, cf) = ("ghp_CanaryPatValue456", "cf-canary-token-789");
    let o = env
        .keel("token", &["--json", "up", "--token-stdin"])
        .env("CLOUDFLARE_API_TOKEN", cf)
        .write_stdin(format!("{pat}\n"))
        .output()
        .unwrap();
    let (out, err) = (
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    );
    assert_eq!(o.status.code(), Some(0), "{out} {err}");
    let v = one_json(&out);
    let ids: Vec<(String, String)> = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["id"].as_str().unwrap().into(),
                s["action"].as_str().unwrap().into(),
            )
        })
        .collect();
    for id in ["token", "deploy", "dns", "backup", "merge", "verify"] {
        assert!(
            ids.iter().any(|(i, a)| i == id && a != "not-reached"),
            "{id}: {ids:?}"
        );
    }
    assert!(
        env.read("stdin.log").contains(pat),
        "the PAT goes to gh stdin"
    );
    let calls = env.read("calls.log");
    for want in ["fly-setup.sh", "--area cloudflare", "gh pr view", "curl "] {
        assert!(calls.contains(want), "{want}: {calls}");
    }
    let mut texts = vec![
        ("stdout".to_string(), out),
        ("stderr".to_string(), err),
        ("calls.log".to_string(), calls),
        ("stdin.log (cf)".to_string(), env.read("stdin.log")),
    ];
    for f in ["keel.toml", "hoist.toml"] {
        texts.push((f.into(), std::fs::read_to_string(env.repo.join(f)).unwrap()));
    }
    for e in std::fs::read_dir(env.repo.join(".keel"))
        .into_iter()
        .flatten()
    {
        let p = e.unwrap().path();
        texts.push((
            format!("{}", p.display()),
            std::fs::read_to_string(&p).unwrap_or_default(),
        ));
    }
    for (what, text) in &texts {
        assert!(!text.contains(cf), "Cloudflare token leaked into {what}");
        if what != "stdin.log (cf)" {
            assert!(!text.contains(pat), "PAT leaked into {what}");
        }
    }
}

#[test]
fn cloudflare_dns_and_backup_use_the_area_flag_and_the_fly_sink() {
    let env = Env::new();
    fly_repo(&env);
    let manifest = format!(
        "{FLY_MANIFEST}dns = \"cloudflare\"\n[backup]\nprovider = \"r2\"\naccount_id = \"0123456789abcdef0123456789abcdef\"\n"
    );
    std::fs::write(env.repo.join("keel.toml"), manifest).unwrap();
    let mut cmd = env.keel("deploy", &["--json", "up"]);
    let o = cmd
        .env("CLOUDFLARE_API_TOKEN", "dummy-not-a-secret")
        .output()
        .unwrap();
    let (out, err) = (
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    );
    assert_eq!(o.status.code(), Some(5), "{out} {err}");
    let calls = env.read("calls.log");
    assert!(
        calls.contains("hoist --json apply --yes --area cloudflare --secrets-to fly:my-app"),
        "{calls}"
    );
    assert!(
        calls.contains("hoist --json check --area github --area npm --area crates --area release"),
        "{calls}"
    );
    assert!(
        !calls.contains("--only") && !calls.contains("--skip"),
        "{calls}"
    );
    assert!(!calls.contains("dummy-not-a-secret") && !out.contains("dummy-not-a-secret"));
    let t = std::fs::read_to_string(env.repo.join("hoist.toml")).unwrap();
    assert!(
        t.contains("[cloudflare.dns]") && t.contains("targets = \"fly\""),
        "{t}"
    );
    assert!(
        t.contains("[cloudflare.r2]")
            && t.contains("account_id = \"0123456789abcdef0123456789abcdef\""),
        "{t}"
    );
}
