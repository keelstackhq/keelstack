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
    [ "$KEEL_SCENARIO" = refuse ] && { echo '{"ok":false,"command":"update","changes":[],"messages":["`apps/site/astro.config.mjs` is owned by channel decisions but was edited locally","nothing was written; revert the files above or pass --force to replace them"]}'; exit 1; }
    mkdir -p docs/agents; echo "decisions v2" > docs/agents/org-decisions.md
    echo '{"ok":true,"changes":[{"path":"docs/agents/org-decisions.md"}]}';;
  "splice --json --yes update")
    [ "$KEEL_SCENARIO" = refuse ] || [ "$KEEL_SCENARIO" = refuse-main ] && { echo '{"ok":false,"command":"update","changes":[],"messages":["`crates/x/AGENTS.md` is owned by the template but was edited locally","nothing was written; revert the files above or pass --force to replace them"]}'; exit 1; }
    echo template v2 > .splice/template.txt; echo '{"ok":true,"changes":[]}';;
  "splice --json --yes update --all")
    [ "$KEEL_SCENARIO" = refuse ] && { echo '{"ok":false,"command":"update","changes":[],"messages":["main: `crates/x/AGENTS.md` is owned by the template but was edited locally","channel decisions: `apps/site/astro.config.mjs` is owned by channel decisions but was edited locally","nothing was written; revert the files above or pass --force to replace them (broken markers are fixed by hand)"]}'; exit 1; }
    [ "$KEEL_SCENARIO" = refuse-main ] && { echo '{"ok":false,"command":"update","changes":[],"messages":["main: `crates/x/AGENTS.md` is owned by the template but was edited locally","nothing was written; revert the files above or pass --force to replace them"]}'; exit 1; }
    [ "$KEEL_SCENARIO" = refuse-other ] && { echo '{"ok":false,"command":"update","changes":[],"messages":["main: `other.md` is owned by the template but was edited locally","nothing was written"]}'; exit 1; }
    [ "$KEEL_SCENARIO" = conflict ] && { echo "<<<<<<< ours" > AGENTS.md; echo '{"ok":false,"exit_code":1}'; exit 1; }
    [ -n "$KEEL_REWRITE_GITIGNORE" ] && printf "dist/\\n" > .gitignore
    [ -n "$KEEL_CREATE_ENV" ] && echo "TOKEN=from-the-step" > .env
    echo "template v2" > .splice/template.txt
    echo '{"ok":true,"changes":[{"path":".splice/template.txt"}]}';;
  "splice --json check")
    DRIFT='{"ok":false,"command":"check","changes":[{"path":"crates/x/AGENTS.md","action":"update","class":"owned"},{"path":"apps/site/astro.config.mjs","action":"update","class":"owned"}],"messages":["`crates/x/AGENTS.md` is owned by the template but was edited locally; `splice update --force` restores it, or revert the change","channel hull: `apps/site/astro.config.mjs` is owned by this channel but was deleted; revert the change","channel hull: 1 managed path(s) differ from .splice/channels/hull.toml"]}'
    NEWDRIFT='{"ok":false,"command":"check","changes":[{"path":"crates/x/AGENTS.md","action":"update","class":"owned"},{"path":".splice/template.txt","action":"update","class":"owned"}],"messages":["`.splice/template.txt` is owned by the template but was edited locally"]}'
    [ "$KEEL_SCENARIO" = predrift ] || [ "$KEEL_SCENARIO" = refuse ] || [ "$KEEL_SCENARIO" = refuse-main ] && { echo "$DRIFT"; exit 1; }
    [ "$KEEL_SCENARIO" = sections ] && { echo '{"ok":false,"command":"check","changes":[{"path":"AGENTS.md#decisions","action":"update","class":"owned"}],"messages":[]}'; exit 1; }
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
  "gh auth status"*) ;;
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

    /// Everything the user can see or lose: every file of the checkout (ignored and untracked
    /// ones too), `.git/HEAD`, the index bytes, the status and the stash.
    fn user_state(&self) -> Vec<(String, Vec<u8>)> {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            let mut es: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap())
                .collect();
            es.sort_by_key(|e| e.file_name());
            for e in es {
                let p = e.path();
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                if rel == ".git" {
                    continue;
                }
                if e.file_type().unwrap().is_dir() {
                    walk(root, &p, out);
                } else {
                    out.push((rel, std::fs::read(&p).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.repo, &self.repo, &mut out);
        out.push((
            ".git/HEAD".into(),
            std::fs::read(self.repo.join(".git/HEAD")).unwrap(),
        ));
        out.push((
            ".git/index".into(),
            std::fs::read(self.repo.join(".git/index")).unwrap_or_default(),
        ));
        out.push((
            "status".into(),
            git(
                &self.repo,
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "--ignored",
                    "--untracked-files=all",
                ],
            )
            .into_bytes(),
        ));
        out.push((
            "stash".into(),
            git(&self.repo, &["stash", "list"]).into_bytes(),
        ));
        out
    }

    /// Run keel and assert the user's checkout is byte-identical afterwards.
    fn run_untouched(&self, scenario: &str, args: &[&str]) -> (i32, serde_json::Value) {
        let before = self.user_state();
        let r = self.run(scenario, args);
        let after = self.user_state();
        assert_eq!(before.len(), after.len(), "files appeared or vanished");
        for (b, a) in before.iter().zip(&after) {
            assert_eq!(b.0, a.0);
            assert!(b.1 == a.1, "{} changed", b.0);
        }
        assert!(
            git(&self.repo, &["worktree", "list"]).lines().count() == 1,
            "no worktree is left behind"
        );
        r
    }

    /// A file of the migration branch.
    fn on_branch(&self, branch: &str, path: &str) -> String {
        git(&self.repo, &["show", &format!("{branch}:{path}")])
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
const BR: &str = "keel/migrate-v0.23.0";

fn no_keel_branch(env: &Env) -> bool {
    git(&env.repo, &["branch", "--list", "keel/*"])
        .trim()
        .is_empty()
}

#[test]
fn full_repo_moves_every_piece_on_one_branch_and_never_touches_the_users_checkout() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["command"], "migrate");
    assert_eq!(v["status"], "human");
    assert_eq!(v["changed"], true);
    assert_eq!(v["branch"], BR);
    assert_eq!(v["from_branch"], "main");
    assert_eq!(v["base"], "origin/main");
    assert_eq!(env.branch(), "main", "the user stays on their branch");
    for s in ["tools", "splice", "bosun-pin"] {
        assert_eq!(action(&v, s), "done", "{s}: {v}");
    }
    assert_eq!(action(&v, "hoist"), "ok");
    assert_eq!(action(&v, "verify"), "ok");
    assert_eq!(action(&v, "pr"), "skipped");
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
    // The first `splice --json check` is the baseline before any step; verify is the last one.
    let last_check = calls
        .iter()
        .rposition(|c| c.starts_with("splice --json check"))
        .unwrap();
    assert!(at("bosun --json pin --to 0.23.0") < last_check);
    assert!(at("splice --json check") < at("splice --json --yes update --channel decisions"));
    let log = git(&env.repo, &["log", "--format=%s", &format!("main..{BR}")]);
    assert!(log.contains("chore: sync org decisions"), "{log}");
    assert!(
        log.contains("chore: pin keelstack tools to 0.23.0"),
        "{log}"
    );
    assert_eq!(env.on_branch(BR, ".bosun/version").trim(), "0.23.0");
    assert_eq!(ids(&v), ["open-pr"]);
    assert_eq!(v["setup_snippet"]["status"], "unchanged");
    assert_eq!(
        git(&env.repo, &["rev-parse", "main"]).trim(),
        git(&env.repo, &["rev-parse", "origin/main"]).trim()
    );
}

#[test]
fn pr_flag_pushes_from_the_worktree_and_opens_one_pr_with_gh() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0", "--pr"]);
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
    assert!(create[0].contains(&format!("--head {BR}")), "{}", create[0]);
    let log = std::fs::read_to_string(env.log.join("calls.log")).unwrap();
    assert!(log.contains("--base main"), "{log}");
    assert!(
        create[0].contains("--title chore: migrate to keelstack v0.23.0"),
        "{}",
        create[0]
    );
    assert!(
        git(&env.repo, &["ls-remote", "--heads", "origin"]).contains(&format!("refs/heads/{BR}"))
    );
    assert!(
        !calls
            .iter()
            .any(|c| c.contains("--force") || c.starts_with("gh pr merge"))
    );
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0", "--pr"]);
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
fn run_first_without_pr_then_with_pr_resumes_the_branch_and_opens_the_pr() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
    let tip = git(&env.repo, &["rev-parse", BR]);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["pr"]["action"], "opened", "{v}");
    assert_eq!(
        git(&env.repo, &["rev-parse", BR]),
        tip,
        "no new commits: every step was a no-op"
    );
}

#[test]
fn a_second_run_on_an_up_to_date_repo_changes_nothing_and_exits_zero() {
    let env = Env::new(&ALL);
    let (code, _) = env.run("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5);
    git(&env.repo, &["merge", "--ff-only", BR]);
    git(&env.repo, &["push", "origin", "main"]);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["status"], "done");
    assert_eq!(v["changed"], false);
    assert_eq!(v["branch"], serde_json::Value::Null);
    assert!(no_keel_branch(&env));
    assert_eq!(action(&v, "pr"), "skipped");
    assert_eq!(v["checklist"].as_array().unwrap().len(), 0);
}

#[test]
fn a_repo_with_only_splice_skips_bosun_and_hoist() {
    let env = Env::new(&["splice"]);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
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
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(action(&v, "splice"), "skipped");
    assert_eq!(action(&v, "bosun-pin"), "done");
    assert!(
        !env.calls()
            .iter()
            .any(|c| c.starts_with("splice ") || c.starts_with("hoist "))
    );
    assert_eq!(
        env.on_branch(BR, "docs/setup-script.sh").trim(),
        "BOSUN_VERSION=0.23.0"
    );
}

#[test]
fn a_changed_setup_snippet_is_written_next_to_the_git_dir_not_into_the_checkout() {
    let env = Env::new(&ALL);
    let before = env.user_state();
    let o = env
        .keel("ok", &["--to", "0.23.0"])
        .env("KEEL_SNIPPET", "changes")
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(o.status.code(), Some(5), "{v}");
    assert_eq!(
        env.user_state().len(),
        before.len(),
        "nothing appeared in the checkout"
    );
    assert_eq!(v["setup_snippet"]["status"], "changed");
    let path = v["setup_snippet"]["path"].as_str().unwrap().to_string();
    assert!(
        path.ends_with("/keel/cloud-setup.sh") && path.contains(".git"),
        "{path}"
    );
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
    assert!(entry["title"].as_str().unwrap().contains(&path));
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("echo SNIPPET v2")
    );
    let prev = path.replace("cloud-setup.sh", "cloud-setup.previous.sh");
    assert!(
        std::fs::read_to_string(prev)
            .unwrap()
            .contains("SNIPPET v1")
    );
    assert!(!env.repo.join(".keel").exists());
}

#[test]
fn an_unchanged_snippet_asks_for_no_paste() {
    let env = Env::new(&ALL);
    let (_, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(v["setup_snippet"]["status"], "unchanged");
    assert!(!ids(&v).contains(&"setup-snippet".to_string()));
}

#[test]
fn dry_run_writes_nothing_installs_nothing_and_reports_the_work() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("dry", &["--to", "0.23.0", "--dry-run", "--pr"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["status"], "planned");
    assert_eq!(v["dry_run"], true);
    assert_eq!(action(&v, "tools"), "would-run");
    assert_eq!(action(&v, "splice"), "would-run");
    assert_eq!(action(&v, "bosun-pin"), "would-run");
    assert!(no_keel_branch(&env));
    assert!(!env.log.join("ver_splice").exists(), "nothing installed");
    let calls = env.calls();
    for banned in [
        "npm install",
        "npm pack",
        "splice --json --yes update",
        "gh pr create",
    ] {
        assert!(
            !calls.iter().any(|c| c.starts_with(banned)),
            "{banned}: {calls:?}"
        );
    }
    assert!(calls.iter().all(|c| c != "bosun --json pin --to 0.23.0"));
}

#[test]
fn a_failing_step_leaves_the_users_checkout_byte_identical_and_names_the_recovery() {
    let env = Env::new(&ALL);
    write(&env.repo, ".gitignore", "local.env\nnode_modules/\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "ignore"]);
    git(&env.repo, &["push", "origin", "main"]);
    write(&env.repo, "local.env", "SECRET=1\n");
    write(&env.repo, "node_modules/pkg/index.js", "x\n");
    write(&env.repo, "notes.txt", "untracked\n");
    write(&env.repo, "README.md", "edited, not committed\n");
    let (code, v) = env.run_untouched("conflict", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["status"], "failed");
    assert_eq!(action(&v, "splice"), "failed");
    assert_eq!(action(&v, "bosun-pin"), "not-reached");
    assert_eq!(action(&v, "pr"), "not-reached");
    let rec = v["recovery"].as_str().unwrap();
    assert!(
        rec.contains("rolled back")
            && rec.contains("never touched")
            && rec.contains("keel migrate"),
        "{rec}"
    );
    assert!(no_keel_branch(&env));
    assert_eq!(
        std::fs::read_to_string(env.repo.join("local.env")).unwrap(),
        "SECRET=1\n"
    );
}

#[test]
fn a_failing_verification_also_rolls_everything_back() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("checkfail", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(action(&v, "verify"), "failed");
    assert!(no_keel_branch(&env));
}

#[test]
fn a_failed_tool_install_fails_the_first_step() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("npmfail", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(action(&v, "tools"), "failed");
    assert!(no_keel_branch(&env));
}

#[test]
fn hoist_drift_becomes_a_computer_step_not_a_failure() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("hoistdrift", &["--to", "0.23.0", "--pr"]);
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
    assert_eq!(list[0]["id"], "hoist-apply");
    assert_eq!(list.last().unwrap()["id"], "approve-pr");
}

#[test]
fn a_dirty_checkout_is_fine_and_stays_exactly_as_it_was() {
    let env = Env::new(&ALL);
    write(&env.repo, "README.md", "edited, not committed\n");
    write(&env.repo, "scratch/untracked.txt", "mine\n");
    git(&env.repo, &["config", "status.showUntrackedFiles", "no"]);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["changed"], true);
    assert_eq!(
        std::fs::read_to_string(env.repo.join("README.md")).unwrap(),
        "edited, not committed\n"
    );
}

#[test]
fn a_bad_target_is_exit_2_and_a_repo_without_pieces_is_exit_3() {
    let env = Env::new(&ALL);
    let (code, v) = env.run("ok", &["--to", "latest"]);
    assert_eq!(code, 2, "{v}");
    let empty = Env::new(&[]);
    let (code, v) = empty.run_untouched("ok", &[]);
    assert_eq!(code, 3, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("nothing to migrate")
    );
    assert!(no_keel_branch(&empty));
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
fn a_mode_that_cannot_be_planned_is_a_warning_not_a_rollback() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("modefail", &["--to", "0.23.0", "--mode", "booster"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["changed"], true);
    assert_eq!(action(&v, "mode"), "pending");
    assert_eq!(v["branch"], BR);
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
fn text_output_has_steps_and_a_numbered_checklist() {
    let env = Env::new(&ALL);
    let c = env.keel("ok", &["--to", "0.23.0"]);
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
        env.on_branch("keel/migrate-v0.22.9", ".bosun/version")
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
fn a_subdirectory_checkout_works_too() {
    let env = Env::new(&ALL);
    write(&env.repo, "sub/x.txt", "x\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "sub"]);
    git(&env.repo, &["push", "origin", "main"]);
    let sub = env.repo.join("sub");
    let o = env
        .keel_in(&sub, "ok", &["--to", "0.23.0"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(o.status.code(), Some(5), "{v}");
    assert_eq!(v["changed"], true);
}

#[test]
fn a_feature_branch_with_unpushed_commits_keeps_them_out_of_the_pr_and_stays_intact() {
    let env = Env::new(&ALL);
    git(&env.repo, &["checkout", "-b", "feature/x"]);
    write(&env.repo, "feature.txt", "unpushed work\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "feature work, not pushed"]);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["from_branch"], "feature/x");
    assert_eq!(v["base"], "origin/main");
    assert_eq!(v["excluded_commits"], 1);
    let log = git(
        &env.repo,
        &["log", "--format=%s", &format!("origin/main..{BR}")],
    );
    assert!(!log.contains("feature work"), "{log}");
    assert!(log.contains("chore: pin keelstack tools"), "{log}");
    let remote = env.repo.parent().unwrap().join("remote.git");
    assert!(
        !git(&remote, &["log", "--format=%s", &format!("main..{BR}")]).contains("feature work")
    );
    assert_eq!(env.branch(), "feature/x");
    assert!(env.repo.join("feature.txt").exists());
}

#[test]
fn a_failure_on_a_feature_branch_leaves_it_and_its_commit_alone() {
    let env = Env::new(&ALL);
    git(&env.repo, &["checkout", "-b", "feature/x"]);
    write(&env.repo, "feature.txt", "unpushed work\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "feature work"]);
    let head = env.head();
    let (code, v) = env.run_untouched("checkfail", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(env.head(), head);
    assert_eq!(env.branch(), "feature/x");
}

#[test]
fn the_base_may_track_a_file_the_users_checkout_ignores() {
    let env = Env::new(&ALL);
    // origin/main tracks .env; the user's own branch untracked it and ignores it, with other content.
    write(&env.repo, ".env", "SHARED=1\n");
    git(&env.repo, &["add", "-f", ".env"]);
    git(&env.repo, &["commit", "-m", "track .env"]);
    git(&env.repo, &["push", "origin", "main"]);
    git(&env.repo, &["rm", "--cached", "-q", ".env"]);
    write(&env.repo, ".gitignore", ".env\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "untrack .env, ignore it"]);
    write(&env.repo, ".env", "LOCAL_SECRET=hunter2\n");
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(
        std::fs::read_to_string(env.repo.join(".env")).unwrap(),
        "LOCAL_SECRET=hunter2\n"
    );
    assert!(!git(&env.repo, &["show", &format!("{BR}:.env")]).contains("hunter2"));
}

#[test]
fn a_step_that_unignores_a_file_it_created_fails_without_committing_it() {
    let env = Env::new(&ALL);
    write(&env.repo, ".gitignore", ".env\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "ignore .env"]);
    git(&env.repo, &["push", "origin", "main"]);
    let o = env
        .keel("ok", &["--to", "0.23.0", "--pr"])
        .env("KEEL_REWRITE_GITIGNORE", "1")
        .env("KEEL_CREATE_ENV", "1")
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(o.status.code(), Some(1), "{v}");
    let detail = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "splice")
        .unwrap()["detail"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        detail.contains(".env") && detail.contains("un-ignore"),
        "{detail}"
    );
    assert!(
        git(&env.repo, &["log", "--all", "--format=%H", "--", ".env"])
            .trim()
            .is_empty()
    );
    assert!(no_keel_branch(&env));
    assert!(!env.calls().iter().any(|c| c.starts_with("gh pr create")));
}

#[test]
fn being_on_the_migration_branch_is_refused_with_the_way_out() {
    let env = Env::new(&ALL);
    git(&env.repo, &["checkout", "-b", BR]);
    let (code, v) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 3, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains(BR), "{v}");
}

#[test]
fn a_failed_fetch_is_exit_3_with_a_fix_and_never_a_stale_base() {
    let env = Env::new(&ALL);
    git(
        &env.repo,
        &["remote", "set-url", "origin", "/nonexistent/remote.git"],
    );
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 3, "{v}");
    assert!(
        v["error"]["message"].as_str().unwrap().contains("fix:"),
        "{v}"
    );
    assert!(no_keel_branch(&env));
    // An explicit base works offline.
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0", "--base", "main"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["base"], "main");
}

#[test]
fn a_feature_branch_ahead_of_origin_with_nothing_to_migrate_opens_no_pr() {
    let env = Env::new(&ALL);
    let (code, _) = env.run("ok", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5);
    git(&env.repo, &["merge", "--ff-only", BR]);
    git(&env.repo, &["push", "origin", "main"]);
    git(&env.repo, &["branch", "-D", BR]);
    git(&env.repo, &["checkout", "-b", "feature/x"]);
    write(&env.repo, "feature.txt", "work\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "feature work, not pushed"]);
    let before = env
        .calls()
        .iter()
        .filter(|c| c.starts_with("gh pr create"))
        .count();
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0", "--pr"]);
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
}

#[test]
fn a_squash_merged_migration_branch_is_replaced_cleanly() {
    let env = Env::new(&ALL);
    let (code, _) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5);
    git(&env.repo, &["merge", "--squash", BR]);
    git(&env.repo, &["commit", "-m", "squash of the migration"]);
    git(&env.repo, &["push", "origin", "main"]);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["changed"], false);
    assert!(no_keel_branch(&env));
}

#[test]
fn a_leftover_branch_is_replaced_when_its_pr_merged_and_never_when_it_has_extra_commits() {
    let env = Env::new(&ALL);
    let (code, _) = env.run("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 5);
    // main moves on (another change lands): the branch is not merged and now behind the base.
    write(&env.repo, "other.txt", "x\n");
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "other"]);
    git(&env.repo, &["push", "origin", "main"]);
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 3, "{v}");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(
        msg.contains("behind") && msg.contains("git branch -D"),
        "{msg}"
    );
    // Its PR is merged, but the branch has a commit beyond the PR's head: exit 3.
    let tip = git(&env.repo, &["rev-parse", BR]).trim().to_string();
    std::fs::write(
        env.log.join("pr_merged"),
        "0000000000000000000000000000000000000000",
    )
    .unwrap();
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0"]);
    assert_eq!(code, 3, "{v}");
    assert!(
        v["error"]["message"].as_str().unwrap().contains("beyond"),
        "{v}"
    );
    assert!(!no_keel_branch(&env));
    // The PR head is the branch tip: the branch is spent and replaced.
    std::fs::write(env.log.join("pr_merged"), &tip).unwrap();
    let (code, v) = env.run_untouched("ok", &["--to", "0.23.0"]);
    assert_ne!(code, 3, "{v}");
    assert_ne!(v["status"], "failed", "{v}");
}

fn entry<'a>(v: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no checklist entry {id}: {v}"))
}

/// Commit `files` to main and push, so the base already has them.
fn commit_base(env: &Env, files: &[(&str, &str)]) {
    for (p, t) in files {
        write(&env.repo, p, t);
    }
    git(&env.repo, &["add", "-A"]);
    git(&env.repo, &["commit", "-m", "base"]);
    git(&env.repo, &["push", "origin", "main"]);
}

#[test]
fn pre_existing_splice_drift_does_not_fail_the_migration_and_is_listed_with_choices() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("predrift", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["status"], "human");
    assert_eq!(action(&v, "verify"), "pending");
    assert!(
        v["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == "verify")
            .unwrap()["detail"]
            .as_str()
            .unwrap()
            .contains("already on the base")
    );
    // The migration itself went on: commits, a PR.
    assert_eq!(v["changed"], true);
    assert_eq!(v["pr"]["action"], "opened");
    let e = entry(&v, "splice-drift");
    assert_eq!(e["device"], "computer");
    let t = e["title"].as_str().unwrap();
    for needle in [
        "crates/x/AGENTS.md",
        "apps/site/astro.config.mjs",
        "owned",
        "edited locally",
        "deleted",
        "splice update --channel hull --exclude apps/site/astro.config.mjs",
        "splice update --channel hull --force",
        "splice update --force",
        "seed file",
    ] {
        assert!(t.contains(needle), "missing {needle:?} in {t}");
    }
    assert_eq!(e["command"], "splice check");
    assert_eq!(v["checklist"][0]["id"], "splice-drift");
}

#[test]
fn drift_a_step_introduced_still_fails_and_rolls_back() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("predrift-new", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(action(&v, "verify"), "failed");
    let d = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "verify")
        .unwrap()["detail"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(d.contains(".splice/template.txt"), "{d}");
    assert!(d.contains("introduced: .splice/template.txt)"), "{d}");
    assert!(no_keel_branch(&env));
}

#[test]
fn hoist_with_nothing_to_publish_is_skipped_without_a_checklist_entry() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("nopublish", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(action(&v, "hoist"), "skipped");
    let d = v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "hoist")
        .unwrap()["detail"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(d.contains("nothing to publish"), "{d}");
    assert!(!ids(&v).contains(&"hoist-init".to_string()), "{v}");
}

#[test]
fn another_hoist_init_failure_shows_hoists_fix_and_never_suggests_the_failing_command() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("hoistfail", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(action(&v, "hoist"), "skipped");
    let e = entry(&v, "hoist-init");
    assert!(
        e["title"]
            .as_str()
            .unwrap()
            .contains("set GH_TOKEN and retry")
    );
    assert_eq!(e["command"], "");
}

#[test]
fn no_commits_but_drift_exits_5_with_the_checklist_and_no_pr() {
    let env = Env::new(&["splice"]);
    commit_base(
        &env,
        &[
            (".splice/template.txt", "template v2\n"),
            ("docs/agents/org-decisions.md", "decisions v2\n"),
        ],
    );
    let (code, v) = env.run_untouched("predrift", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["status"], "human");
    assert_eq!(v["changed"], false);
    assert_eq!(v["pr"], serde_json::Value::Null);
    assert_eq!(v["branch"], serde_json::Value::Null);
    assert_eq!(action(&v, "pr"), "skipped");
    assert_eq!(ids(&v), vec!["splice-drift"]);
    assert!(no_keel_branch(&env));
    assert!(!env.calls().iter().any(|c| c.starts_with("gh pr create")));
}

#[test]
fn no_commits_but_a_changed_setup_snippet_exits_5_with_no_pr() {
    let env = Env::new(&["bosun"]);
    commit_base(
        &env,
        &[
            (".bosun/version", "0.23.0\n"),
            ("docs/setup-script.sh", "BOSUN_VERSION=0.23.0\n"),
        ],
    );
    let o = env
        .keel("ok", &["--to", "0.23.0", "--pr"])
        .env("KEEL_SNIPPET", "changes")
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(o.status.code(), Some(5), "{v}");
    assert_eq!(v["status"], "human");
    assert_eq!(v["changed"], false);
    assert_eq!(v["pr"], serde_json::Value::Null);
    assert_eq!(ids(&v), vec!["setup-snippet"]);
    assert!(!env.calls().iter().any(|c| c.starts_with("gh pr create")));
}

#[test]
fn splice_refusing_owned_drift_marks_the_source_not_updated_and_the_run_goes_on() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("refuse-main", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(action(&v, "splice"), "pending");
    // the other source and the later steps still ran and were committed
    assert_ne!(action(&v, "bosun-pin"), "failed");
    assert_eq!(v["changed"], true);
    assert_eq!(v["pr"]["action"], "opened");
    let log = git(&env.repo, &["log", "--format=%s", &format!("main..{BR}")]);
    assert!(log.contains("chore: sync org decisions"), "{log}");
    let t = entry(&v, "splice-drift")["title"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(t.contains("NOT updated"), "{t}");
    assert!(t.contains("main template"), "{t}");
    assert!(t.contains("then rerun `keel migrate`"), "{t}");
}

#[test]
fn every_source_refusing_leaves_no_commits_and_no_pr() {
    let env = Env::new(&["splice"]);
    let (code, v) = env.run_untouched("refuse", &["--to", "0.23.0", "--pr"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["status"], "human");
    assert_eq!(v["pr"], serde_json::Value::Null);
    assert_eq!(v["changed"], false);
    let t = entry(&v, "splice-drift")["title"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        t.contains("main template") && t.contains("channel decisions"),
        "{t}"
    );
}

#[test]
fn a_refusal_for_a_path_that_was_not_drifted_on_the_base_fails_and_rolls_back() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("refuse-other", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(action(&v, "splice"), "failed");
    assert!(no_keel_branch(&env));
}

#[test]
fn section_drift_belongs_to_its_channel_and_never_suggests_exclude() {
    let env = Env::new(&["splice"]);
    commit_base(
        &env,
        &[
            (".splice/template.txt", "template v2\n"),
            ("docs/agents/org-decisions.md", "decisions v2\n"),
        ],
    );
    let (code, v) = env.run_untouched("sections", &["--to", "0.23.0"]);
    assert_eq!(code, 5, "{v}");
    let t = entry(&v, "splice-drift")["title"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        t.contains("AGENTS.md#decisions") && t.contains("channel decisions"),
        "{t}"
    );
    assert!(
        t.contains("splice update --channel decisions --force"),
        "{t}"
    );
    assert!(!t.contains("--exclude AGENTS.md#decisions"), "{t}");
}

#[test]
fn drift_on_the_same_path_with_a_new_cause_counts_as_introduced() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("cause-change", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(action(&v, "verify"), "failed");
}

#[test]
fn a_baseline_check_that_errors_is_noted_and_treated_as_unknown() {
    let env = Env::new(&ALL);
    let (code, v) = env.run_untouched("basebroken", &["--to", "0.23.0"]);
    assert_eq!(code, 1, "{v}");
    let notes = v["notes"].as_array().unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.as_str().unwrap().contains("baseline")),
        "{v}"
    );
}
