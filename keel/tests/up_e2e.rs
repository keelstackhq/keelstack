//! `keel up` end to end, upgrade path (K5): a real git repo and bare remote, with stub `npm`,
//! `splice`, `bosun`, `hoist` and `gh` scripts first on PATH (no real tool, no network). The stubs
//! keep their state in `$KEEL_LOG`. UC-K5-1: a 0.13.0 base to 0.25.0 autonomous with one agent
//! command, one owner command and one owner approval. UC-K5-2: a private Free-org repo stops at
//! preflight before any write.
use assert_cmd::Command;
use base64::Engine;
use sha2::{Digest, Sha512};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

const STUB: &str = r#"#!/bin/sh
name=$(basename "$0")
echo "$name $*" >> "$KEEL_LOG/calls.log"
ver() { cat "$KEEL_LOG/ver_$1" 2>/dev/null || echo 0.13.0; }
env() { printf '{"version":1,"command":"x","ok":true,"exit_code":0,"data":%s,"error":null,"next":[]}\n' "$1"; }
case "$name $*" in
  "npm view "*) printf 'sha512-%s\n' "$KEEL_INTEGRITY";;
  "npm pack "*)
    spec=${3#@keelstackhq/}; t=${spec%@*}; v=${spec#*@}
    f="keelstackhq-$t-$v.tgz"; printf TARBALL > "$f"; echo "$f";;
  "npm install "*)
    [ "$KEEL_SCENARIO" = npmfail ] && { echo "npm ERR! EACCES" >&2; exit 1; }
    for a in "$@"; do case "$a" in *.tgz)
      b=$(basename "$a" .tgz); v=${b##*-}; r=${b%-*}; t=${r#keelstackhq-}
      case "$t" in splice|bosun|hoist) echo "$v" > "$KEEL_LOG/ver_$t";; esac;;
    esac; done;;
  "splice --version") echo "splice $(ver splice)";;
  "bosun --version") echo "bosun $(ver bosun)";;
  "hoist --version") echo "hoist $(ver hoist)";;
  "splice --json --yes update --channel decisions")
    mkdir -p docs/agents; echo "decisions v2" > docs/agents/org-decisions.md
    echo '{"ok":true,"changes":[{"path":"docs/agents/org-decisions.md"}]}';;
  "splice --json --yes update --all")
    [ "$KEEL_SCENARIO" = conflict ] && { echo "<<<<<<< ours" > AGENTS.md; echo '{"ok":false,"exit_code":1}'; exit 1; }
    [ -n "$KEEL_REWRITE_GITIGNORE" ] && printf "dist/\\n" > .gitignore
    [ -n "$KEEL_CREATE_ENV" ] && echo "TOKEN=from-the-step" > .env
    echo "template v2" > .splice/template.txt
    echo '{"ok":true,"changes":[{"path":".splice/template.txt"}]}';;
  "splice --json check")
    DRIFT='{"ok":false,"command":"check","changes":[{"path":"crates/x/AGENTS.md","action":"update","class":"owned"},{"path":"apps/site/astro.config.mjs","action":"update","class":"owned"}],"messages":["`crates/x/AGENTS.md` is owned by the template but was edited locally; `splice update --force` restores it, or revert the change","channel hull: `apps/site/astro.config.mjs` is owned by this channel but was deleted; revert the change","channel hull: 1 managed path(s) differ from .splice/channels/hull.toml"]}'
    NEWDRIFT='{"ok":false,"command":"check","changes":[{"path":"crates/x/AGENTS.md","action":"update","class":"owned"},{"path":".splice/template.txt","action":"update","class":"owned"}],"messages":["`.splice/template.txt` is owned by the template but was edited locally"]}'
    [ "$KEEL_SCENARIO" = predrift ] && { echo "$DRIFT"; exit 1; }
    [ "$KEEL_SCENARIO" = cause-change ] && { if grep -q "template v2" .splice/template.txt 2>/dev/null; then echo '{"ok":false,"command":"check","changes":[{"path":"crates/x/AGENTS.md","action":"update","class":"owned"}],"messages":["`crates/x/AGENTS.md` is owned by the template but was deleted"]}'; else echo '{"ok":false,"command":"check","changes":[{"path":"crates/x/AGENTS.md","action":"update","class":"owned"}],"messages":["`crates/x/AGENTS.md` is owned by the template but was edited locally"]}'; fi; exit 1; }
    [ "$KEEL_SCENARIO" = basebroken ] && { if grep -q "template v2" .splice/template.txt 2>/dev/null; then echo "$DRIFT"; exit 1; else echo "boom" >&2; exit 3; fi; }
    [ "$KEEL_SCENARIO" = predrift-new ] && { if grep -q "template v2" .splice/template.txt 2>/dev/null; then echo "$NEWDRIFT"; else echo "$DRIFT"; fi; exit 1; }
    [ "$KEEL_SCENARIO" = checkfail ] && { echo '{"ok":false,"exit_code":1}'; exit 1; }
    [ "$KEEL_SCENARIO" = dry ] && { echo '{"ok":false,"exit_code":1}'; exit 1; }
    echo '{"ok":true,"exit_code":0}';;
  "bosun --json pin --to "*" --check")
    env '{"behind":[{"tool":"bosun","from":"0.22.1","to":"0.23.0"}],"unchanged":[]}';;
  "bosun --json pin --to "*)
    echo "$4" > .bosun/version; echo "BOSUN_VERSION=$4" > docs/setup-script.sh
    env '{"changed":[{"tool":"bosun","from":"0.22.1","to":"0.23.0"}],"unchanged":[],"files":["docs/setup-script.sh",".bosun/version"]}';;
  "bosun --json setup-snippet")
    if [ "$KEEL_SNIPPET" = changes ] && [ "$(ver bosun)" = 0.23.0 ]; then
      env '{"script":"echo SNIPPET v2\nexport A=1"}'
    else env '{"script":"echo SNIPPET v1"}'; fi;;
  "bosun --json doctor")
    [ "$KEEL_SCENARIO" = offline ] && { echo '{"ok":false,"exit_code":5,"data":{"offline":true}}'; exit 5; }
    env '{}';;
  "bosun --json migrate --to"*)
    [ "$KEEL_SCENARIO" = modefail ] && { echo "bosun: boom" >&2; exit 3; }
    env '{"planned":[]}';;
  "hoist --json init"*)
    [ "$KEEL_SCENARIO" = nopublish ] && { echo '{"version":1,"command":"init","ok":false,"exit_code":3,"items":[],"human_steps":[],"error":{"message":"no publishable package found; nothing to write (see `hoist explain`)","fix":"the message says what to change","kind":"nothing-to-publish"}}'; exit 3; }
    [ "$KEEL_SCENARIO" = hoistfail ] && { echo '{"version":1,"command":"init","ok":false,"exit_code":3,"items":[],"human_steps":[],"error":{"message":"GitHub API unreachable","fix":"set GH_TOKEN and retry"}}'; exit 3; }
    echo '{"ok":true,"exit_code":0,"items":[],"human_steps":[]}';;
  "hoist --json check")
    [ "$KEEL_SCENARIO" = hoistdrift ] && { echo '{"ok":false,"exit_code":1,"items":[{"area":"github","kind":"environment","key":"release-tags","action":"create","reason":"missing"}],"human_steps":[]}'; exit 1; }
    echo '{"ok":true,"exit_code":0,"items":[],"human_steps":[]}';;
  "gh auth status"*) echo "  - Token scopes: 'repo', 'workflow'";;
  "gh api user --jq .login") echo octo;;
  "gh api repos/owner/name")
    echo "{\"private\":${KEEL_PRIVATE:-false},\"owner\":{\"login\":\"owner\",\"type\":\"Organization\"},\"permissions\":{\"admin\":true}}";;
  "gh api orgs/owner") echo "{\"plan\":{\"name\":\"${KEEL_PLAN:-team}\"}}";;
  "bosun --json gate-app advance")
    [ "$KEEL_SCENARIO" = old-bosun ] && { echo "unknown command" >&2; exit 2; }
    if [ -f "$KEEL_LOG/owner_done" ]; then
      env '{"dry_run":false,"app_id":42,"slug":"gate","step":"pr-b","status":"advanced","steps":[],"left":[],"branch":"bosun/gate-app","pr":{"number":11,"head_sha":"abc"}}'
    else
      env '{"dry_run":false,"app_id":42,"slug":"gate","step":"pr-a","status":"advanced","steps":[],"left":[],"branch":"bosun/gate-app","pr":{"number":10,"head_sha":"abc"}}'
    fi;;
  "gh repo view"*) echo '{"nameWithOwner":"owner/name","defaultBranchRef":{"name":"main"}}';;
  "gh pr view"*)
    [ -f "$KEEL_LOG/pr_merged" ] && { printf '{"number":9,"url":"https://github.com/owner/name/pull/9","state":"MERGED","headRefOid":"%s","mergeCommit":{"oid":"c0"}}\n' "$(cat "$KEEL_LOG/pr_merged")"; exit 0; }
    [ -f "$KEEL_LOG/pr_open" ] && echo '{"number":9,"url":"https://github.com/owner/name/pull/9","state":"OPEN","headRefOid":"abc","mergeCommit":null}' || exit 1;;
  "gh pr create"*) : > "$KEEL_LOG/pr_open"; echo "https://github.com/owner/name/pull/9";;
esac
exit 0
"#;

struct Env {
    _root: tempfile::TempDir,
    stubs: PathBuf,
    log: PathBuf,
    repo: PathBuf,
    home: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

impl Env {
    /// The 0.13.0 base: splice, bosun and hoist pieces; `extra` is written to keel.toml when given.
    fn new(lock: bool, keel_toml: Option<&str>) -> Env {
        let root = tempfile::tempdir().unwrap();
        let p = |n: &str| {
            let d = root.path().join(n);
            std::fs::create_dir(&d).unwrap();
            d
        };
        let (stubs, log, repo, home, remote) =
            (p("stubs"), p("log"), p("repo"), p("home"), p("remote.git"));
        let script = stubs.join("stub.sh");
        std::fs::write(&script, STUB).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        for t in ["npm", "splice", "bosun", "hoist", "gh"] {
            symlink(&script, stubs.join(t)).unwrap();
        }
        std::fs::write(root.path().join("pins.toml"), "[pins]\n").unwrap();
        git(&remote, &["init", "--bare", "-b", "main"]);
        git(&repo, &["init", "-b", "main"]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        if lock {
            write(&repo, ".splice/lock.toml", "version = \"0.13.0\"\n");
            write(&repo, ".splice/channels/decisions.toml", "x = 1\n");
        }
        write(&repo, ".bosun/version", "0.13.0\n");
        write(&repo, "docs/setup-script.sh", "BOSUN_VERSION=0.13.0\n");
        write(&repo, "hoist.toml", "[github]\n");
        write(&repo, ".github/workflows/release.yml", "name: release\n");
        write(&repo, "README.md", "hi\n");
        if let Some(k) = keel_toml {
            write(&repo, "keel.toml", k);
        }
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-m", "init"]);
        git(&repo, &["push", "-u", "origin", "main"]);
        Env {
            _root: root,
            stubs,
            log,
            repo,
            home,
        }
    }

    fn cmd(&self, envs: &[(&str, &str)], args: &[&str]) -> Command {
        let integrity =
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(b"TARBALL"));
        let mut c = Command::cargo_bin("keel").unwrap();
        c.env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.stubs.display()))
            .env("HOME", &self.home)
            .env("TMPDIR", &self.home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .env("KEEL_LOG", &self.log)
            .env("KEEL_PINS", self._root.path().join("pins.toml"))
            .env("KEEL_SCENARIO", "ok")
            .env("KEEL_INTEGRITY", integrity)
            .arg("-C")
            .arg(&self.repo)
            .arg("--json")
            .args(args);
        for (k, v) in envs {
            c.env(k, v);
        }
        c
    }

    fn run(&self, envs: &[(&str, &str)], args: &[&str]) -> (i32, serde_json::Value) {
        let o = self.cmd(envs, args).output().unwrap();
        let out = String::from_utf8(o.stdout).unwrap();
        let v = serde_json::from_str(out.trim()).unwrap_or_else(|e| {
            panic!(
                "one JSON object: {e}: {out}\n{}",
                String::from_utf8_lossy(&o.stderr)
            )
        });
        (o.status.code().unwrap(), v)
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log.join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn called(&self, prefix: &str) -> usize {
        self.calls()
            .iter()
            .filter(|c| c.starts_with(prefix))
            .count()
    }
}

fn step(v: &serde_json::Value, id: &str) -> serde_json::Value {
    v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("no step {id}: {v}"))
        .clone()
}

fn ids(v: &serde_json::Value) -> Vec<String> {
    v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_string())
        .collect()
}

const OWNER_LINE: &str = "npx -y @keelstackhq/keel@0.25.0 owner";

#[test]
fn uc_k5_1_a_013_base_reaches_025_autonomous_with_one_agent_command_one_owner_command_one_approval()
{
    let env = Env::new(true, None);
    // The one agent command.
    let (code, v) = env.run(&[], &["up", "--to", "0.25.0", "--mode", "autonomous"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["command"], "up");
    assert_eq!(v["data"]["path"], "upgrade");
    assert!(
        v["data"]["reason"].as_str().unwrap().contains("lock.toml"),
        "{v}"
    );
    assert_eq!(v["data"]["target"], "0.25.0");
    assert_eq!(step(&v, "pr")["action"], "done");
    assert_eq!(step(&v, "mode-preset")["action"], "done");
    assert_eq!(step(&v, "gate-app")["action"], "done");
    assert!(
        step(&v, "gate-app")["detail"]
            .as_str()
            .unwrap()
            .contains("#10"),
        "{v}"
    );
    // Exactly one owner command and one owner approval (PR A) are left.
    assert_eq!(ids(&v), ["owner", "approve-pr"], "{v}");
    assert_eq!(v["human_step"]["command"], OWNER_LINE);
    assert_eq!(v["checklist"][0]["command"], OWNER_LINE);
    assert_eq!(v["checklist"][1]["device"], "phone");
    // Order: the PR first, then the mode preset, then gate-app advance.
    let calls = env.calls();
    let at = |n: &str| {
        calls
            .iter()
            .position(|c| c.starts_with(n))
            .unwrap_or_else(|| panic!("{n}: {calls:?}"))
    };
    assert!(at("gh pr create") < at("bosun --json migrate --to autonomous"));
    assert!(at("bosun --json migrate --to autonomous") < at("bosun --json gate-app advance"));
    // The PR body lists each step and the owner line.
    let log = std::fs::read_to_string(env.log.join("calls.log")).unwrap();
    assert!(log.contains("## keel up"), "{log}");
    assert!(log.contains(OWNER_LINE), "{log}");
    assert!(log.contains("gate-app advance"), "{log}");
    assert_eq!(env.called("gh pr create"), 1);
    // Never merges, never forces.
    assert!(
        !calls
            .iter()
            .any(|c| c.starts_with("gh pr merge") || c.contains("--force "))
    );

    // The owner runs their one command; the agent reruns `keel up`: PR B, no owner line again.
    std::fs::write(env.log.join("owner_done"), "").unwrap();
    let (_, v) = env.run(&[], &["up", "--to", "0.25.0", "--mode", "autonomous"]);
    assert_eq!(v["data"]["path"], "upgrade");
    assert_eq!(step(&v, "gate-app")["action"], "done");
    assert!(
        step(&v, "gate-app")["detail"]
            .as_str()
            .unwrap()
            .contains("#11"),
        "{v}"
    );
    assert!(!ids(&v).contains(&"owner".to_string()), "{v}");
    assert_eq!(env.called("gh pr create"), 1, "no second migration PR");
}

#[test]
fn uc_k5_2_a_private_free_org_repo_stops_at_preflight_before_any_write() {
    let env = Env::new(true, Some("repo = \"owner/name\"\n"));
    let (code, v) = env.run(
        &[("KEEL_PRIVATE", "true"), ("KEEL_PLAN", "free")],
        &["up", "--to", "0.25.0"],
    );
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["status"], "block");
    assert_eq!(v["findings"][0]["id"], "plan-free-private");
    let calls = env.calls();
    for bad in [
        "git fetch",
        "npm ",
        "splice ",
        "gh pr create",
        "bosun --json migrate",
        "bosun --json gate-app",
        "bosun --json pin",
    ] {
        assert!(
            !calls.iter().any(|c| c.starts_with(bad)),
            "{bad}: {calls:?}"
        );
    }
    assert!(
        git(&env.repo, &["branch", "--list", "keel/*"])
            .trim()
            .is_empty()
    );
    assert!(
        git(&env.repo, &["ls-remote", "--heads", "origin"])
            .lines()
            .count()
            == 1
    );
}

#[test]
fn ci_provider_none_lifts_the_free_private_block_and_the_upgrade_goes_on() {
    let env = Env::new(
        true,
        Some("repo = \"owner/name\"\n[ci]\nprovider = \"none\"\n"),
    );
    let (code, v) = env.run(
        &[("KEEL_PRIVATE", "true"), ("KEEL_PLAN", "free")],
        &["up", "--to", "0.25.0"],
    );
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["data"]["path"], "upgrade", "{v}");
    assert_eq!(v["command"], "up");
    assert_eq!(env.called("gh pr create"), 1);
}

#[test]
fn no_lock_file_is_the_bootstrap_path_and_new_forces_it() {
    // No `.splice/lock.toml`: bootstrap runs (it needs a repo and a template, so it stops there).
    let env = Env::new(false, None);
    let (code, v) = env.run(&[], &["up"]);
    assert_eq!(code, 2, "{v}");
    assert!(
        v["error"]["message"].as_str().unwrap().contains("no repo"),
        "{v}"
    );
    assert_eq!(env.called("gh pr create"), 0);
    // `--new` forces the same bootstrap although the lock file exists.
    let env = Env::new(true, None);
    let (code, v) = env.run(&[], &["up", "--new"]);
    assert_eq!(code, 2, "{v}");
    assert!(
        v["error"]["message"].as_str().unwrap().contains("no repo"),
        "{v}"
    );
    assert_eq!(env.called("bosun --json gate-app"), 0);
}

#[test]
fn bootstrap_rejects_the_upgrade_only_flags() {
    let env = Env::new(false, None);
    let (code, v) = env.run(&[], &["up", "--to", "0.25.0"]);
    assert_eq!(code, 2, "{v}");
    assert!(
        v["error"]["message"].as_str().unwrap().contains("--to"),
        "{v}"
    );
}

#[test]
fn existing_forces_the_upgrade_and_needs_the_lock_file() {
    let env = Env::new(false, None);
    let (code, v) = env.run(&[], &["up", "--existing"]);
    assert_eq!(code, 2, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("lock.toml"),
        "{v}"
    );
    assert_eq!(env.called("npm "), 0);
}

#[test]
fn new_and_existing_conflict() {
    let env = Env::new(true, None);
    let o = env
        .cmd(&[], &["up", "--new", "--existing"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn to_sets_the_target_release() {
    let env = Env::new(true, None);
    let (_, v) = env.run(&[], &["up", "--to", "0.24.1"]);
    assert_eq!(v["data"]["target"], "0.24.1");
    assert_eq!(
        v["human_step"]["command"],
        "npx -y @keelstackhq/keel@0.24.1 owner"
    );
    assert!(env.called("bosun --json pin --to 0.24.1") > 0);
}

#[test]
fn mode_other_than_autonomous_runs_the_preset_only() {
    let env = Env::new(true, None);
    let (code, v) = env.run(&[], &["up", "--to", "0.25.0", "--mode", "steady"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(env.called("bosun --json migrate --to steady"), 1);
    assert_eq!(env.called("bosun --json gate-app"), 0);
    assert_eq!(ids(&v), ["approve-pr"], "{v}");
}

#[test]
fn branch_pushes_to_that_branch_and_opens_no_pr() {
    let env = Env::new(true, None);
    let (_, v) = env.run(&[], &["up", "--to", "0.25.0", "--branch", "claude/up-x"]);
    assert_eq!(env.called("gh pr create"), 0, "{v}");
    assert!(git(&env.repo, &["ls-remote", "--heads", "origin"]).contains("refs/heads/claude/up-x"));
    assert!(env.called("git push origin HEAD:refs/heads/claude/up-x") <= 1);
    assert_eq!(step(&v, "gate-app")["action"], "done");
}

#[test]
fn dry_run_writes_pushes_and_advances_nothing() {
    let env = Env::new(true, None);
    let (code, v) = env.run(&[], &["up", "--to", "0.25.0", "--dry-run"]);
    assert_eq!(v["data"]["path"], "upgrade", "{v}");
    assert_eq!(v["data"]["dry_run"], true);
    assert!(code == 0 || code == 1, "{code} {v}");
    assert_eq!(env.called("gh pr create"), 0);
    assert_eq!(env.called("bosun --json migrate --to"), 0);
    assert_eq!(env.called("bosun --json gate-app"), 0);
    assert_eq!(step(&v, "mode-preset")["action"], "would-run");
    assert_eq!(step(&v, "gate-app")["action"], "would-run");
    assert!(
        git(&env.repo, &["branch", "--list", "keel/*"])
            .trim()
            .is_empty()
    );
}

#[test]
fn an_old_bosun_without_gate_app_degrades_to_a_pending_step_not_a_crash() {
    let env = Env::new(true, None);
    let (code, v) = env.run(&[("KEEL_SCENARIO", "old-bosun")], &["up", "--to", "0.25.0"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(step(&v, "gate-app")["action"], "pending");
    assert!(ids(&v).contains(&"owner".to_string()), "{v}");
}
