//! K1 T2: `keel migrate --dry-run` on a machine without splice, bosun or hoist fetches them into a
//! temp cache (stub `npm`, checksums verified) and lists the full plan; `--branch NAME` pushes to
//! that branch. Stub scripts on PATH, no real tool, no network.
use assert_cmd::Command;
use base64::Engine;
use sha2::{Digest, Sha512};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

/// One script, linked as npm, gh, splice, bosun, hoist. `npm install --prefix D` links the tools into
/// `D/node_modules/.bin`; they live in `$KEEL_PKGS`, never on PATH.
const STUB: &str = r#"#!/bin/sh
name=$(basename "$0")
echo "$name $*" >> "$KEEL_LOG/calls.log"
case "$name $*" in
  "npm view "*) printf 'sha512-%s\n' "$KEEL_INTEGRITY";;
  "npm pack "*)
    spec=${3#@keelstackhq/}; t=${spec%@*}; v=${spec#*@}
    f="keelstackhq-$t-$v.tgz"; printf "$KEEL_TARBALL" > "$f"; echo "$f";;
  "npm install -g"*) echo "global install" >&2; exit 1;;
  "npm install --prefix "*)
    mkdir -p "$3/node_modules/.bin"
    for t in splice bosun hoist; do ln -sf "$KEEL_PKGS/$t" "$3/node_modules/.bin/$t"; done;;
  "splice --version") echo "splice 0.22.0";;
  "bosun --version") echo "bosun 0.22.0";;
  "hoist --version") echo "hoist 0.22.0";;
  "splice --json check")
    [ -f .splice/template.txt ] && { echo '{"ok":true,"exit_code":0}'; exit 0; }
    echo '{"ok":false,"exit_code":1}'; exit 1;;
  "splice --json --yes update"*)
    mkdir -p .splice; echo "template v2" > .splice/template.txt
    echo '{"ok":true,"changes":[{"path":".splice/template.txt"}]}';;
  "bosun --json setup-snippet") echo '{"ok":true,"exit_code":0,"data":{"script":"echo SNIPPET"}}';;
  "gh "*) echo "gh: not logged in" >&2; exit 1;;
  "bosun --json doctor"|"hoist --json check") echo '{"ok":true,"exit_code":0,"data":{},"items":[],"human_steps":[]}';;
esac
exit 0
"#;

struct Env {
    root: tempfile::TempDir,
    stubs: PathBuf,
    log: PathBuf,
    repo: PathBuf,
    remote: PathBuf,
    home: PathBuf,
    tarball: &'static str,
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

fn script(path: &Path) {
    std::fs::write(path, STUB).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

impl Env {
    /// `tools_on_path`: splice, bosun and hoist are installed (on PATH) or not.
    fn new(tools_on_path: bool) -> Env {
        let root = tempfile::tempdir().unwrap();
        let p = |n: &str| {
            let d = root.path().join(n);
            std::fs::create_dir(&d).unwrap();
            d
        };
        let (stubs, pkgs, log, repo, home, remote) = (
            p("stubs"),
            p("pkgs"),
            p("log"),
            p("repo"),
            p("home"),
            p("remote.git"),
        );
        let main = root.path().join("stub.sh");
        script(&main);
        symlink(&main, stubs.join("npm")).unwrap();
        // A real gh would write its state under $HOME; the stub says "not logged in".
        symlink(&main, stubs.join("gh")).unwrap();
        for t in ["splice", "bosun", "hoist"] {
            symlink(&main, pkgs.join(t)).unwrap();
            if tools_on_path {
                symlink(&main, stubs.join(t)).unwrap();
            }
        }
        std::fs::write(root.path().join("pins.toml"), "[pins]\n").unwrap();
        git(&remote, &["init", "--bare", "-b", "main"]);
        git(&repo, &["init", "-b", "main"]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        for (rel, text) in [
            (".splice/lock.toml", "version = \"0.1.0\"\n"),
            (".bosun/version", "0.22.0\n"),
            ("hoist.toml", "[github]\n"),
            (".github/workflows/release.yml", "name: release\n"),
            ("README.md", "hi\n"),
        ] {
            let f = repo.join(rel);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        }
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-m", "init"]);
        git(&repo, &["push", "-u", "origin", "main"]);
        Env {
            root,
            stubs,
            log,
            repo,
            remote,
            home,
            tarball: "TARBALL",
        }
    }

    fn keel(&self, args: &[&str]) -> Command {
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
            .env("KEEL_PKGS", self.root.path().join("pkgs"))
            .env("KEEL_PINS", self.root.path().join("pins.toml"))
            .env("KEEL_INTEGRITY", integrity)
            .env("KEEL_TARBALL", self.tarball)
            .arg("-C")
            .arg(&self.repo)
            .arg("--json")
            .arg("migrate")
            .args(args);
        c
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.log.join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

fn json_of(o: &std::process::Output) -> serde_json::Value {
    let out = String::from_utf8_lossy(&o.stdout);
    serde_json::from_str(out.trim()).unwrap_or_else(|e| {
        panic!(
            "one JSON object: {e}: {out}\n{}",
            String::from_utf8_lossy(&o.stderr)
        )
    })
}

fn step<'a>(v: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("no step {id}: {v}"))
}

/// UC-K1-2: no splice, bosun or hoist installed: the dry-run still lists the whole plan.
#[test]
fn dry_run_without_tools_fetches_them_and_lists_the_plan() {
    let e = Env::new(false);
    let o = e.keel(&["--dry-run", "--to", "0.22.0"]).output().unwrap();
    let v = json_of(&o);
    assert_eq!(v["dry_run"], true, "{v}");
    for s in v["steps"].as_array().unwrap() {
        assert_ne!(s["action"], "failed", "{v}");
        assert_ne!(s["action"], "not-reached", "{v}");
    }
    assert_eq!(step(&v, "tools")["action"], "would-run", "{v}");
    assert_eq!(step(&v, "splice")["action"], "would-run", "{v}");
    // The plan came from the fetched tools, with the checksums verified through npm pack/install.
    let calls = e.calls();
    assert!(calls.iter().any(|c| c.starts_with("npm pack")), "{calls:?}");
    assert!(
        calls.iter().any(|c| c.starts_with("npm install --prefix")),
        "{calls:?}"
    );
    assert!(
        calls.iter().any(|c| c == "splice --json check"),
        "{calls:?}"
    );
    // Nothing global, nothing left behind, nothing written to the repo.
    assert!(!calls.iter().any(|c| c.starts_with("npm install -g")));
    assert_eq!(git(&e.repo, &["status", "--porcelain"]), "");
    assert_eq!(git(&e.repo, &["branch", "--list"]).trim(), "* main");
    let left: Vec<_> = std::fs::read_dir(&e.home)
        .unwrap()
        .map(|d| d.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "temp cache left behind: {left:?}");
}

/// The same checksum verification as the real install: a wrong tarball fails the plan.
#[test]
fn dry_run_fetch_refuses_a_tarball_with_the_wrong_checksum() {
    let mut e = Env::new(false);
    e.tarball = "TAMPERED";
    let o = e.keel(&["--dry-run", "--to", "0.22.0"]).output().unwrap();
    let v = json_of(&o);
    assert_eq!(step(&v, "tools")["action"], "failed", "{v}");
    assert!(
        step(&v, "tools")["detail"]
            .as_str()
            .unwrap()
            .contains("sha512 mismatch"),
        "{v}"
    );
    assert!(
        !e.calls()
            .iter()
            .any(|c| c.starts_with("npm install --prefix"))
    );
}

/// `--branch NAME`: commit and push to NAME, no keel/migrate-v* branch, plain push.
#[test]
fn branch_flag_pushes_to_the_named_branch() {
    let e = Env::new(true);
    git(&e.repo, &["switch", "-c", "claude/session"]);
    let o = e
        .keel(&["--branch", "claude/session", "--to", "0.22.0"])
        .output()
        .unwrap();
    let v = json_of(&o);
    assert_eq!(v["branch"], "claude/session", "{v}");
    assert_eq!(v["changed"], true, "{v}");
    let heads = git(&e.remote, &["branch", "--list"]);
    assert!(heads.contains("claude/session"), "{heads}");
    assert!(!heads.contains("keel/migrate"), "{heads}");
    let msg = git(&e.remote, &["log", "-1", "--format=%s", "claude/session"]);
    assert!(msg.contains("splice update"), "{msg}");
    assert_eq!(git(&e.repo, &["branch", "--list", "keel/*"]).trim(), "");
    let _ = e.stubs;
}

/// A rejected (non fast-forward) push fails; keel never forces.
#[test]
fn branch_flag_never_forces_the_push() {
    let e = Env::new(true);
    git(&e.repo, &["switch", "-c", "claude/session"]);
    std::fs::write(e.repo.join("other.txt"), "x").unwrap();
    git(&e.repo, &["add", "-A"]);
    git(&e.repo, &["commit", "-m", "remote only"]);
    git(&e.repo, &["push", "origin", "claude/session"]);
    git(&e.repo, &["reset", "--hard", "main"]);
    let o = e
        .keel(&["--branch", "claude/session", "--to", "0.22.0"])
        .output()
        .unwrap();
    let v = json_of(&o);
    assert_eq!(step(&v, "pr")["action"], "failed", "{v}");
    let msg = git(&e.remote, &["log", "-1", "--format=%s", "claude/session"]);
    assert!(msg.contains("remote only"), "{msg}");
}
