//! `keel migrate --drift seed|take|keep` against the REAL `splice` built from this workspace, on a
//! fixture shaped like the anu base: 4 drifted paths (two owned AGENTS.md with local lines, a
//! deleted channel file, an edited channel file). No network, no npm, bosun, hoist or gh.
use assert_cmd::Command;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn splice_bin() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let out = std::process::Command::new(cargo)
            .args([
                "build",
                "-p",
                "keel-splice",
                "--locked",
                "--message-format=json",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("cargo build -p keel-splice");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|v| v["reason"] == "compiler-artifact" && v["target"]["name"] == "splice")
            .filter_map(|v| v["executable"].as_str().map(PathBuf::from))
            .next_back()
            .expect("the splice executable")
    })
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

fn splice(repo: &Path, args: &[&str]) {
    let o = std::process::Command::new(splice_bin())
        .arg("-C")
        .arg(repo)
        .args(["--yes", "--json"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "splice {args:?}: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

const OWNED: [&str; 2] = ["AGENTS.md", "svc/AGENTS.md"];

fn tpl_text(path: &str, n: &str) -> String {
    format!("# {path}\nrule one {n}\nrule two {n}\n")
}

/// Owned `AGENTS.md` files with their seed `CLAUDE.md` next to each; v1.0.0 and v2.0.0.
fn template(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    write(
        dir,
        "splice.toml",
        "format = 1\nname = \"t\"\nmin_splice = \"0.2.0\"\nbase = \"base\"\n",
    );
    write(
        dir,
        "pieces/base/piece.toml",
        "id = \"base\"\ndescription = \"b\"\nmultiple = false\n\n[files]\nowned = [\"AGENTS.md\", \"svc/AGENTS.md\"]\nseed = [\"CLAUDE.md\", \"svc/CLAUDE.md\"]\n",
    );
    for (n, tag) in [("one", "v1.0.0"), ("two", "v2.0.0")] {
        for p in OWNED {
            write(dir, &format!("pieces/base/files/{p}"), &tpl_text(p, n));
        }
        write(dir, "pieces/base/files/CLAUDE.md", "# seed\n");
        write(dir, "pieces/base/files/svc/CLAUDE.md", "# svc seed\n");
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", n]);
        git(dir, &["tag", tag]);
    }
}

fn channel(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    write(
        dir,
        "channel/splice.toml",
        "format = 1\nname = \"rig\"\nmin_splice = \"0.2.0\"\nbase = \"core\"\n",
    );
    write(
        dir,
        "channel/pieces/core/piece.toml",
        "id = \"core\"\ndescription = \"c\"\nmultiple = false\n\n[files]\nowned = [\"docs/a.md\", \"docs/b.md\"]\n",
    );
    for (n, tag) in [("one", "v1.0.0"), ("two", "v2.0.0")] {
        for f in ["a", "b"] {
            write(
                dir,
                &format!("channel/pieces/core/files/docs/{f}.md"),
                &format!("{f} {n}\n"),
            );
        }
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", n]);
        git(dir, &["tag", tag]);
    }
}

const LOCAL_MAIN: [(&str, &[&str]); 2] = [
    ("AGENTS.md", &["local note A1", "local note A2"]),
    ("svc/AGENTS.md", &["svc local note"]),
];
const LOCAL_CHANNEL_EDIT: &str = "my own b\n";

struct World {
    root: tempfile::TempDir,
    repo: PathBuf,
}

impl World {
    fn new() -> World {
        let root = tempfile::tempdir().unwrap();
        let mk = |n: &str| {
            let d = root.path().join(n);
            std::fs::create_dir(&d).unwrap();
            d
        };
        let (tpl, ch, remote, repo) = (mk("tpl"), mk("ch"), mk("remote.git"), mk("repo"));
        template(&tpl);
        channel(&ch);
        git(&remote, &["init", "--bare", "-q", "-b", "main"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        write(&repo, "README.md", "hi\n");
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        splice(
            &repo,
            &[
                "new",
                tpl.to_str().unwrap(),
                ".",
                "--ref",
                "v1.0.0",
                "--force",
            ],
        );
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "template"]);
        splice(
            &repo,
            &[
                "channel",
                "add",
                &format!("{}//channel", ch.display()),
                "--as",
                "rig",
                "--ref",
                "v1.0.0",
            ],
        );
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "channel"]);
        // The four drifted paths.
        for (p, lines) in LOCAL_MAIN {
            let mut t = std::fs::read_to_string(repo.join(p)).unwrap();
            for l in lines {
                t.push_str(l);
                t.push('\n');
            }
            std::fs::write(repo.join(p), t).unwrap();
        }
        std::fs::remove_file(repo.join("docs/a.md")).unwrap();
        std::fs::write(repo.join("docs/b.md"), LOCAL_CHANNEL_EDIT).unwrap();
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "local edits"]);
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        World { root, repo }
    }

    fn keel(&self, extra: &[&str]) -> (i32, Value) {
        std::fs::write(self.root.path().join("pins.toml"), "[pins]\n").unwrap();
        let bin_dir = splice_bin().parent().unwrap().to_path_buf();
        let o = Command::cargo_bin("keel")
            .unwrap()
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", bin_dir.display()))
            .env("HOME", self.root.path())
            .env("TMPDIR", self.root.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .env("KEEL_PINS", self.root.path().join("pins.toml"))
            .arg("-C")
            .arg(&self.repo)
            .args(["--json", "migrate", "--to", env!("CARGO_PKG_VERSION")])
            .args(extra)
            .output()
            .unwrap();
        let out = String::from_utf8(o.stdout).unwrap();
        let v = serde_json::from_str(out.trim()).unwrap_or_else(|e| {
            panic!(
                "one JSON object: {e}: {out}\n{}",
                String::from_utf8_lossy(&o.stderr)
            )
        });
        (o.status.code().unwrap(), v)
    }

    fn branch_file(&self, rel: &str) -> String {
        let b = format!("keel/migrate-v{}", env!("CARGO_PKG_VERSION"));
        git(&self.repo, &["show", &format!("{b}:{rel}")])
    }
}

fn checklist_ids(v: &Value) -> Vec<String> {
    v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_string())
        .collect()
}

fn drift_of<'a>(v: &'a Value, path: &str) -> &'a Value {
    v["drift"]
        .as_array()
        .unwrap_or_else(|| panic!("no drift array: {v}"))
        .iter()
        .find(|d| d["path"] == path)
        .unwrap_or_else(|| panic!("no drift entry for {path}: {v}"))
}

#[test]
fn seed_moves_local_lines_to_the_seed_files_and_excludes_channel_drift() {
    let w = World::new();
    let (code, v) = w.keel(&[]);
    assert!(
        !checklist_ids(&v).contains(&"splice-drift".to_string()),
        "{v}"
    );
    assert_eq!(code, 5, "only the open-pr step is left: {v}");
    assert_eq!(v["drift"].as_array().unwrap().len(), 4, "{v}");

    // Owned files are the template's v2; every removed local line is in the seed file.
    for (p, lines) in LOCAL_MAIN {
        assert_eq!(w.branch_file(p), tpl_text(p, "two"), "{p}");
        let seed = w.branch_file(&p.replace("AGENTS.md", "CLAUDE.md"));
        for l in lines {
            assert!(seed.contains(l), "{l} lost: {seed}");
        }
        let d = drift_of(&v, p);
        assert_eq!(d["action"], "moved");
        assert_eq!(d["moved_lines"], lines.len());
    }
    // Channel drift: both paths are excluded, the local text stays.
    assert_eq!(drift_of(&v, "docs/a.md")["action"], "excluded");
    assert_eq!(drift_of(&v, "docs/b.md")["action"], "excluded");
    let lock = w.branch_file(".splice/channels/rig.toml");
    assert!(
        lock.contains("docs/a.md") && lock.contains("docs/b.md"),
        "{lock}"
    );
    assert_eq!(w.branch_file("docs/b.md"), LOCAL_CHANNEL_EDIT);
}

#[test]
fn take_restores_the_template_and_lists_each_path() {
    let w = World::new();
    let (_, v) = w.keel(&["--drift", "take"]);
    assert!(
        !checklist_ids(&v).contains(&"splice-drift".to_string()),
        "{v}"
    );
    for p in OWNED {
        assert_eq!(w.branch_file(p), tpl_text(p, "two"));
        assert_eq!(drift_of(&v, p)["action"], "taken");
    }
    assert_eq!(w.branch_file("docs/b.md"), "b two\n");
}

#[test]
fn keep_excludes_channel_files_and_leaves_owned_main_files_for_the_owner() {
    let w = World::new();
    let (_, v) = w.keel(&["--drift", "keep"]);
    assert_eq!(drift_of(&v, "docs/b.md")["action"], "excluded");
    assert_eq!(w.branch_file("docs/b.md"), LOCAL_CHANNEL_EDIT);
    assert!(
        checklist_ids(&v).contains(&"splice-drift".to_string()),
        "{v}"
    );
}

#[test]
fn an_unknown_mode_is_a_usage_error() {
    let w = World::new();
    let o = std::process::Command::new(assert_cmd::cargo::cargo_bin("keel"))
        .args(["migrate", "--drift", "bogus"])
        .current_dir(&w.repo)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
}
