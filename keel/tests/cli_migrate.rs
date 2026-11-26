//! `keel migrate` end to end: a real git repo and bare remote, with stub `npm`, `splice`, `bosun`,
//! `hoist` and `gh` scripts first on PATH (no real tool, no network). The stubs keep their state in
//! `$KEEL_LOG`, so a second run sees what the first one did.
use assert_cmd::Command;
use base64::Engine;
use sha2::{Digest, Sha512};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

const STUB: &str = r#"#!/bin/sh
name=$(basename "$0")
echo "$name $*" >> "$KEEL_LOG/calls.log"
ver() { cat "$KEEL_LOG/ver_$1" 2>/dev/null || echo 0.22.0; }
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
    echo "template v2" > .splice/template.txt
    echo '{"ok":true,"changes":[{"path":".splice/template.txt"}]}';;
  "splice --json check")
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
  "hoist --json init"*) echo '{"ok":true,"exit_code":0,"items":[],"human_steps":[]}';;
  "hoist --json check")
    [ "$KEEL_SCENARIO" = hoistdrift ] && { echo '{"ok":false,"exit_code":1,"items":[{"area":"github","kind":"environment","key":"release-tags","action":"create","reason":"missing"}],"human_steps":[]}'; exit 1; }
    echo '{"ok":true,"exit_code":0,"items":[],"human_steps":[]}';;
  "gh auth status"*) ;;
  "gh repo view"*) echo '{"nameWithOwner":"owner/name","defaultBranchRef":{"name":"main"}}';;
  "gh pr view"*)
    [ -f "$KEEL_LOG/pr_merged" ] && { echo '{"number":9,"url":"https://github.com/owner/name/pull/9","state":"MERGED","headRefOid":"abc","mergeCommit":{"oid":"c0"}}'; exit 0; }
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
    /// `pieces`: any of splice, bosun, hoist.
    fn new(pieces: &[&str]) -> Env {
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
        // No sha512 for any version: keel takes the registry's integrity (the stub npm's).
        std::fs::write(root.path().join("pins.toml"), "[pins]\n").unwrap();
        git(&remote, &["init", "--bare", "-b", "main"]);
        git(&repo, &["init", "-b", "main"]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        if pieces.contains(&"splice") {
            write(&repo, ".splice/lock.toml", "version = \"0.1.0\"\n");
            write(&repo, ".splice/channels/decisions.toml", "x = 1\n");
        }
        if pieces.contains(&"bosun") {
            write(&repo, ".bosun/version", "0.22.1\n");
            write(&repo, "docs/setup-script.sh", "BOSUN_VERSION=0.22.1\n");
        }
        if pieces.contains(&"hoist") {
            write(&repo, "hoist.toml", "[github]\n");
            write(&repo, ".github/workflows/release.yml", "name: release\n");
        }
        write(&repo, "README.md", "hi\n");
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

    fn keel(&self, scenario: &str, args: &[&str]) -> Command {
        self.keel_in(&self.repo, scenario, args)
    }

    fn keel_in(&self, dir: &Path, scenario: &str, args: &[&str]) -> Command {
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
            .env("KEEL_SCENARIO", scenario)
            .env("KEEL_INTEGRITY", integrity)
            .arg("-C")
            .arg(dir)
            .arg("--json")
            .arg("migrate")
            .args(args);
        c
    }

    fn run(&self, scenario: &str, args: &[&str]) -> (i32, serde_json::Value) {
        let o = self.keel(scenario, args).output().unwrap();
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

    fn head(&self) -> String {
        git(&self.repo, &["rev-parse", "HEAD"]).trim().to_string()
    }

    fn branch(&self) -> String {
        git(&self.repo, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim()
            .to_string()
    }
}

fn action(v: &serde_json::Value, id: &str) -> String {
    v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("no step {id}: {v}"))["action"]
        .as_str()
        .unwrap()
        .to_string()
}

fn ids(v: &serde_json::Value) -> Vec<String> {
    v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_string())
        .collect()
}

const ALL: [&str; 3] = ["splice", "bosun", "hoist"];

#[test]
fn full_repo_moves_every_piece_on_one_branch_and_lists_what_a_human_must_do() {
    let env = Env::new(&ALL);
    let (code, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["command"], "migrate");
    assert_eq!(v["status"], "human");
    assert_eq!(v["changed"], true);
    assert_eq!(v["branch"], "keel/migrate-v0.23.0");
    assert_eq!(env.branch(), "keel/migrate-v0.23.0");
    for s in ["tools", "splice", "bosun-pin"] {
        assert_eq!(action(&v, s), "done", "{s}: {v}");
    }
    assert_eq!(action(&v, "hoist"), "ok");
    assert_eq!(action(&v, "verify"), "ok");
    assert_eq!(action(&v, "pr"), "skipped");
    // Tools were installed at the target (registry integrity, tarball re-checked).
    let calls = env.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("npm view @keelstackhq/splice@0.23.0 dist.integrity"))
    );
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("npm install -g --ignore-scripts"))
    );
    for t in ["splice", "bosun", "hoist"] {
        assert_eq!(
            std::fs::read_to_string(env.log.join(format!("ver_{t}")))
                .unwrap()
                .trim(),
            "0.23.0"
        );
    }
    // Order: decisions alone first, then the rest, then the pin, then verification (read-only).
    let at = |needle: &str| {
        calls
            .iter()
            .position(|c| c.starts_with(needle))
            .unwrap_or_else(|| panic!("{needle}: {calls:?}"))
    };
    assert!(
        at("splice --json --yes update --channel decisions")
            < at("splice --json --yes update --all")
    );
    assert!(at("splice --json --yes update --all") < at("bosun --json pin --to 0.23.0"));
    assert!(at("bosun --json pin --to 0.23.0") < at("splice --json check"));
    // One commit per step, decisions alone with the org's message.
    let log = git(&env.repo, &["log", "--format=%s", "main..HEAD"]);
    assert!(log.contains("chore: sync org decisions"), "{log}");
    assert!(
        log.contains("chore: pin keelstack tools to 0.23.0"),
        "{log}"
    );
    assert_eq!(
        std::fs::read_to_string(env.repo.join(".bosun/version"))
            .unwrap()
            .trim(),
        "0.23.0"
    );
    // Nothing but the PR step is left, and with no --pr it says so.
    assert_eq!(ids(&v), ["open-pr"]);
    assert_eq!(v["human_step"]["id"], "open-pr");
    assert_eq!(v["setup_snippet"]["status"], "unchanged");
    // The main branch is untouched.
    assert_eq!(
        git(&env.repo, &["rev-parse", "main"]).trim(),
        git(&env.repo, &["rev-parse", "origin/main"]).trim()
    );
}

#[test]
fn pr_flag_pushes_and_opens_one_pr_with_gh() {
    let env = Env::new(&ALL);
    let (code, v) = env.run("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["pr"]["action"], "opened");
    assert_eq!(v["pr"]["url"], "https://github.com/owner/name/pull/9");
    assert_eq!(ids(&v), ["approve-pr"]);
    assert_eq!(v["checklist"][0]["device"], "phone");
    let calls = env.calls();
    assert!(calls.contains(&"gh auth status".to_string()));
    let create: Vec<_> = calls
        .iter()
        .filter(|c| c.starts_with("gh pr create"))
        .collect();
    assert_eq!(create.len(), 1, "{calls:?}");
    assert!(create[0].contains("--repo owner/name"), "{}", create[0]);
    assert!(
        create[0].contains("--head keel/migrate-v0.23.0"),
        "{}",
        create[0]
    );
    assert!(
        create[0].contains("--title chore: migrate to keelstack v0.23.0"),
        "{}",
        create[0]
    );
    // The branch is on the remote; no force, no merge.
    assert!(
        git(&env.repo, &["ls-remote", "--heads", "origin"])
            .contains("refs/heads/keel/migrate-v0.23.0")
    );
    assert!(
        !calls
            .iter()
            .any(|c| c.contains("--force") || c.starts_with("gh pr merge"))
    );
    // Rerun with the PR open: reused, not opened twice; everything else is a no-op.
    let (code, v) = env.run("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["pr"]["action"], "existing");
    assert_eq!(
        env.calls()
            .iter()
            .filter(|c| c.starts_with("gh pr create"))
            .count(),
        1
    );
}

#[test]
fn a_second_run_on_an_up_to_date_repo_changes_nothing_and_exits_zero() {
    let env = Env::new(&ALL);
    let (code, _) = env.run("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5);
    // The PR merges: main gets the work.
    git(&env.repo, &["checkout", "main"]);
    git(&env.repo, &["merge", "--ff-only", "keel/migrate-v0.23.0"]);
    git(&env.repo, &["push", "origin", "main"]);
    let before = env.head();
    let (code, v) = env.run("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["status"], "done");
    assert_eq!(v["changed"], false);
    assert_eq!(v["branch"], serde_json::Value::Null);
    assert_eq!(env.head(), before);
    assert_eq!(env.branch(), "main");
    assert!(
        git(&env.repo, &["branch", "--list", "keel/*"])
            .trim()
            .is_empty()
    );
    assert_eq!(action(&v, "pr"), "skipped");
    assert_eq!(v["checklist"].as_array().unwrap().len(), 0);
    assert!(git(&env.repo, &["status", "--porcelain"]).trim().is_empty());
}

#[test]
fn a_repo_with_only_splice_skips_bosun_and_hoist() {
    let env = Env::new(&["splice"]);
    let (code, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["pieces"]["splice"], true);
    assert_eq!(v["pieces"]["bosun"], false);
    assert_eq!(action(&v, "splice"), "done");
    for s in ["bosun-pin", "hoist", "setup-snippet"] {
        assert_eq!(action(&v, s), "skipped", "{s}");
    }
    let calls = env.calls();
    assert!(
        !calls
            .iter()
            .any(|c| c.starts_with("bosun ") || c.starts_with("hoist ")),
        "{calls:?}"
    );
    assert!(!env.log.join("ver_bosun").exists());
}

#[test]
fn a_repo_with_only_bosun_pins_and_checks_the_snippet() {
    let env = Env::new(&["bosun"]);
    let (code, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["pieces"]["splice"], false);
    assert_eq!(action(&v, "splice"), "skipped");
    assert_eq!(action(&v, "bosun-pin"), "done");
    assert!(
        !env.calls()
            .iter()
            .any(|c| c.starts_with("splice ") || c.starts_with("hoist "))
    );
    assert_eq!(
        std::fs::read_to_string(env.repo.join("docs/setup-script.sh"))
            .unwrap()
            .trim(),
        "BOSUN_VERSION=0.23.0"
    );
}

#[test]
fn a_changed_setup_snippet_is_written_and_named_in_the_checklist() {
    let env = Env::new(&ALL);
    let o = env
        .keel("ok", &["--to", "0.23.0"])
        .env("KEEL_SNIPPET", "changes")
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(o.status.code(), Some(5), "{v}");
    assert_eq!(v["setup_snippet"]["status"], "changed");
    assert_eq!(v["setup_snippet"]["path"], ".keel/cloud-setup.sh");
    let summary = v["setup_snippet"]["summary"].as_str().unwrap();
    assert!(summary.contains("lines added"), "{summary}");
    assert!(ids(&v).contains(&"setup-snippet".to_string()), "{v}");
    let entry = v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "setup-snippet")
        .unwrap();
    assert_eq!(entry["device"], "computer");
    assert!(
        entry["title"]
            .as_str()
            .unwrap()
            .contains(".keel/cloud-setup.sh")
    );
    let script = std::fs::read_to_string(env.repo.join(".keel/cloud-setup.sh")).unwrap();
    assert!(script.contains("echo SNIPPET v2"), "{script}");
    assert!(
        std::fs::read_to_string(env.repo.join(".keel/cloud-setup.previous.sh"))
            .unwrap()
            .contains("SNIPPET v1")
    );
    // .keel is scratch space: never committed.
    assert!(!git(&env.repo, &["ls-files"]).contains(".keel"));
}

#[test]
fn an_unchanged_snippet_asks_for_no_paste() {
    let env = Env::new(&ALL);
    let (_, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(v["setup_snippet"]["status"], "unchanged");
    assert!(!ids(&v).contains(&"setup-snippet".to_string()));
    assert!(!env.repo.join(".keel/cloud-setup.sh").exists());
}

#[test]
fn dry_run_writes_nothing_installs_nothing_and_reports_the_work() {
    let env = Env::new(&ALL);
    let head = env.head();
    let (code, v) = env.run("dry", &["--to", "0.23.0", "--dry-run", "--pr"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["status"], "planned", "{v}");
    assert_eq!(v["dry_run"], true);
    assert_eq!(action(&v, "tools"), "would-run");
    assert_eq!(action(&v, "splice"), "would-run");
    assert_eq!(action(&v, "bosun-pin"), "would-run");
    assert_eq!(env.head(), head);
    assert_eq!(env.branch(), "main");
    assert!(git(&env.repo, &["status", "--porcelain"]).trim().is_empty());
    assert!(!env.repo.join(".keel").exists());
    assert!(!env.log.join("ver_splice").exists(), "nothing installed");
    let calls = env.calls();
    for banned in [
        "npm install",
        "npm pack",
        "splice --json --yes update",
        "gh pr create",
    ] {
        assert!(
            !calls.iter().any(|c| c.starts_with(banned.trim())),
            "{banned}: {calls:?}"
        );
    }
    assert!(calls.iter().all(|c| c != "bosun --json pin --to 0.23.0"));
}

#[test]
fn a_failing_step_rolls_back_to_the_untouched_branch_and_says_how_to_recover() {
    let env = Env::new(&ALL);
    let head = env.head();
    let (code, v) = env.run("conflict", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["status"], "failed");
    assert_eq!(action(&v, "splice"), "failed");
    assert_eq!(action(&v, "bosun-pin"), "not-reached");
    assert_eq!(action(&v, "pr"), "not-reached");
    let rec = v["recovery"].as_str().unwrap();
    assert!(
        rec.contains("rolled back") && rec.contains("keel migrate"),
        "{rec}"
    );
    assert_eq!(env.branch(), "main");
    assert_eq!(env.head(), head);
    assert!(
        git(&env.repo, &["branch", "--list", "keel/*"])
            .trim()
            .is_empty()
    );
    assert!(
        git(&env.repo, &["status", "--porcelain"]).trim().is_empty(),
        "no stray files"
    );
    // The conflict marker file the stub wrote is gone, and so is the already-committed decisions sync.
    assert!(!env.repo.join("AGENTS.md").exists());
    assert!(!env.repo.join("docs/agents/org-decisions.md").exists());
}

#[test]
fn a_failing_verification_also_rolls_everything_back() {
    let env = Env::new(&ALL);
    let head = env.head();
    let (code, v) = env.run("checkfail", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(action(&v, "verify"), "failed");
    assert_eq!(env.head(), head);
    assert_eq!(env.branch(), "main");
    assert_eq!(
        std::fs::read_to_string(env.repo.join(".bosun/version"))
            .unwrap()
            .trim(),
        "0.22.1"
    );
}

#[test]
fn a_failed_tool_install_fails_the_first_step_without_touching_git() {
    let env = Env::new(&ALL);
    let head = env.head();
    let (code, v) = env.run("npmfail", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(action(&v, "tools"), "failed");
    assert_eq!(env.head(), head);
    assert_eq!(env.branch(), "main");
}

#[test]
fn hoist_drift_becomes_a_computer_step_not_a_failure() {
    let env = Env::new(&ALL);
    let (code, v) = env.run("hoistdrift", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(action(&v, "verify"), "pending");
    let list = v["checklist"].as_array().unwrap();
    let apply = list.iter().find(|e| e["id"] == "hoist-apply").unwrap();
    assert_eq!(apply["command"], "hoist apply --area github");
    assert!(
        apply["title"]
            .as_str()
            .unwrap()
            .contains("environment/release-tags")
    );
    // Computer steps come before the phone step (approve the PR).
    assert_eq!(list[0]["id"], "hoist-apply");
    assert_eq!(list.last().unwrap()["id"], "approve-pr");
}

#[test]
fn a_dirty_tree_is_refused_with_exit_3_and_a_fix() {
    let env = Env::new(&ALL);
    std::fs::write(env.repo.join("README.md"), "edited\n").unwrap();
    let (code, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 3, "{v}");
    assert_eq!(v["status"], "error");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("working tree has changes")
    );
    assert_eq!(env.branch(), "main");
}

#[test]
fn a_bad_target_is_exit_2_and_a_repo_without_pieces_is_exit_3() {
    let env = Env::new(&ALL);
    let (code, v) = env.run("ok", &["--to", "latest"]);
    assert_eq!(code, 2, "{v}");
    let empty = Env::new(&[]);
    let (code, v) = empty.run("ok", &[]);
    assert_eq!(code, 3, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("nothing to migrate")
    );
}

#[test]
fn mode_is_planned_as_a_step_for_after_the_merge() {
    let env = Env::new(&ALL);
    let (_, v) = env.run("ok", &["--to", "0.23.0", "--mode", "booster"]);
    assert_eq!(action(&v, "mode"), "pending");
    let e = v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "bosun-mode")
        .unwrap();
    assert_eq!(e["command"], "bosun migrate --to booster");
}

#[test]
fn text_output_has_steps_and_a_numbered_checklist() {
    let env = Env::new(&ALL);
    let mut c = env.keel("ok", &["--to", "0.23.0"]);
    // The same command line without --json (the first args are -C DIR --json migrate ...).
    let args: Vec<String> = c
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .filter(|a| a != "--json")
        .collect();
    let mut t = Command::cargo_bin("keel").unwrap();
    t.env_clear();
    for (k, v) in c.get_envs() {
        if let Some(v) = v {
            t.env(k, v);
        }
    }
    let _ = &mut c;
    let o = t.args(&args).output().unwrap();
    assert_eq!(o.status.code(), Some(5));
    let out = String::from_utf8(o.stdout).unwrap();
    assert!(
        out.contains("bosun-pin") && out.contains("checklist:\n  1. [computer]"),
        "{out}"
    );
}

#[test]
fn a_target_that_is_not_npm_latest_is_pinned_exactly() {
    let env = Env::new(&ALL);
    let (code, v) = env.run("ok", &["--to", "0.22.9"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(action(&v, "bosun-pin"), "done", "{v}");
    assert_eq!(
        std::fs::read_to_string(env.repo.join(".bosun/version"))
            .unwrap()
            .trim(),
        "0.22.9"
    );
    let calls = env.calls();
    assert!(
        calls.contains(&"bosun --json pin --to 0.22.9".to_string()),
        "{calls:?}"
    );
    assert!(!calls.iter().any(|c| c.contains("--latest")));
}

#[test]
fn untracked_files_are_found_even_when_git_is_told_to_hide_them() {
    let env = Env::new(&ALL);
    git(&env.repo, &["config", "status.showUntrackedFiles", "no"]);
    write(&env.repo, "notes/todo.txt", "mine\n");
    let (code, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 3, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("working tree has changes"),
        "{v}"
    );
    assert_eq!(
        std::fs::read_to_string(env.repo.join("notes/todo.txt")).unwrap(),
        "mine\n"
    );
    assert_eq!(env.branch(), "main");
}

#[test]
fn a_rollback_never_deletes_a_file_the_user_had_even_an_ignored_one() {
    let env = Env::new(&ALL);
    write(&env.repo, ".gitignore", "local.env\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "ignore"]);
    write(&env.repo, "local.env", "SECRET=1\n");
    write(&env.repo, "dir/ignored-too/local.env", "x\n");
    let (code, v) = env.run("conflict", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(
        std::fs::read_to_string(env.repo.join("local.env")).unwrap(),
        "SECRET=1\n"
    );
    // Only what keel's own failed step created is removed.
    assert!(!env.repo.join("AGENTS.md").exists());
    assert!(env.repo.join("dir/ignored-too/local.env").exists());
    assert!(!env.calls().iter().any(|c| c.starts_with("git clean")));
}

#[test]
fn a_subdirectory_checkout_is_refused_because_the_status_check_covers_the_whole_repo() {
    let env = Env::new(&ALL);
    write(&env.repo, "sub/hoist.toml", "[github]\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "sub"]);
    write(&env.repo, "README.md", "edited at the root\n");
    let sub = env.repo.join("sub");
    let o = env
        .keel_in(&sub, "ok", &["--to", "0.23.0"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(o.status.code(), Some(3), "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("repo root"),
        "{v}"
    );
    assert_eq!(
        std::fs::read_to_string(env.repo.join("README.md")).unwrap(),
        "edited at the root\n"
    );
    assert_eq!(env.branch(), "main");
}

#[test]
fn a_mode_that_cannot_be_planned_is_a_warning_not_a_rollback() {
    let env = Env::new(&ALL);
    let (code, v) = env.run("modefail", &["--to", "0.23.0", "--mode", "booster"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["changed"], true);
    assert_eq!(action(&v, "mode"), "pending");
    assert_eq!(env.branch(), "keel/migrate-v0.23.0");
    let e = v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "bosun-mode")
        .unwrap();
    assert_eq!(e["command"], "bosun migrate --to booster");
    assert!(v["recovery"].is_null());
}

#[test]
fn a_feature_branch_ahead_of_origin_with_nothing_to_migrate_opens_no_pr() {
    let env = Env::new(&ALL);
    let (code, _) = env.run("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5);
    git(&env.repo, &["checkout", "main"]);
    git(&env.repo, &["merge", "--ff-only", "keel/migrate-v0.23.0"]);
    git(&env.repo, &["push", "origin", "main"]);
    git(&env.repo, &["branch", "-D", "keel/migrate-v0.23.0"]);
    git(&env.repo, &["checkout", "-b", "feature/x"]);
    write(&env.repo, "feature.txt", "work\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "feature work, not pushed"]);
    let before = env
        .calls()
        .iter()
        .filter(|c| c.starts_with("gh pr create"))
        .count();
    let (code, v) = env.run("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["changed"], false);
    assert_eq!(action(&v, "pr"), "skipped");
    assert_eq!(
        env.calls()
            .iter()
            .filter(|c| c.starts_with("gh pr create"))
            .count(),
        before
    );
    assert_eq!(env.branch(), "feature/x");
}

#[test]
fn a_squash_merged_migration_branch_is_replaced_cleanly() {
    let env = Env::new(&ALL);
    let (code, _) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5);
    // The PR is squash-merged: main gets the same tree in one new commit; the branch is no ancestor.
    git(&env.repo, &["checkout", "main"]);
    git(&env.repo, &["merge", "--squash", "keel/migrate-v0.23.0"]);
    git(&env.repo, &["commit", "-m", "squash of the migration"]);
    let (code, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["changed"], false);
    assert!(
        git(&env.repo, &["branch", "--list", "keel/*"])
            .trim()
            .is_empty()
    );
    assert_eq!(env.branch(), "main");
}

#[test]
fn a_leftover_branch_whose_pr_is_merged_is_replaced_and_any_other_gets_exact_advice() {
    let env = Env::new(&ALL);
    let (code, _) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5);
    git(&env.repo, &["checkout", "main"]);
    // main moved on in another way (rebase-merge with edits): trees differ, the branch is no ancestor.
    write(&env.repo, "other.txt", "x\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "other"]);
    let (code, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 3, "{v}");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("git switch keel/migrate-v0.23.0") && msg.contains("git branch -D"),
        "{msg}"
    );
    std::fs::write(env.log.join("pr_merged"), "").unwrap();
    let (code, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_ne!(code, 3, "{v}");
    assert_ne!(v["status"], "failed", "{v}");
}
