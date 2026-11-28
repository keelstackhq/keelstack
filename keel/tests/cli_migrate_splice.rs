//! `keel migrate` against the REAL `splice` binary built from this workspace, on small template
//! and channel repos that live on disk (`git` sources, no network). Only the target version is
//! shared with the tool: no npm, bosun, hoist or gh is involved (the repo uses splice only).
use assert_cmd::Command;
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
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
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

/// A main template with the owned file `docs/owned.md`, tagged v1.0.0 ("one") and v2.0.0 ("two").
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
        "id = \"base\"\ndescription = \"b\"\nmultiple = false\n\n[files]\nowned = [\"docs/owned.md\"]\n",
    );
    for (n, tag) in [("one", "v1.0.0"), ("two", "v2.0.0")] {
        write(dir, "pieces/base/files/docs/owned.md", &format!("{n}\n"));
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", n]);
        git(dir, &["tag", tag]);
    }
}

/// A channel `name` (source subdirectory `channel`) owning `docs/agents/<name>.md` and, with
/// `host`, a section block in that Markdown file.
fn channel(dir: &Path, name: &str, host: Option<&str>) {
    git(dir, &["init", "-q", "-b", "main"]);
    write(
        dir,
        "channel/splice.toml",
        &format!("format = 1\nname = \"{name}\"\nmin_splice = \"0.2.0\"\nbase = \"core\"\n"),
    );
    let sections = host
        .map(|h| format!("\n[sections]\n\"{h}\" = \"sections/rules.md\"\n"))
        .unwrap_or_default();
    write(
        dir,
        "channel/pieces/core/piece.toml",
        &format!(
            "id = \"core\"\ndescription = \"c\"\nmultiple = false\n\n[files]\nowned = [\"docs/agents/{name}.md\"]\n{sections}"
        ),
    );
    for (n, tag) in [("one", "v1.0.0"), ("two", "v2.0.0")] {
        write(
            dir,
            &format!("channel/pieces/core/files/docs/agents/{name}.md"),
            &format!("{n}\n"),
        );
        write(
            dir,
            "channel/pieces/core/files/sections/rules.md",
            &format!("rules {n}\n"),
        );
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", n]);
        git(dir, &["tag", tag]);
    }
}

struct World {
    root: tempfile::TempDir,
    repo: PathBuf,
}

impl World {
    /// A repo on `main` (pushed to a bare origin) rendered from the template and the channels
    /// `decisions` (section in AGENTS.md) and `rig`, all at `at`.
    fn new(at: &str) -> World {
        let root = tempfile::tempdir().unwrap();
        let mk = |n: &str| {
            let d = root.path().join(n);
            std::fs::create_dir(&d).unwrap();
            d
        };
        let (tpl, chd, chr, remote, repo) = (
            mk("tpl"),
            mk("chd"),
            mk("chr"),
            mk("remote.git"),
            mk("repo"),
        );
        template(&tpl);
        channel(&chd, "decisions", Some("AGENTS.md"));
        channel(&chr, "rig", None);
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
            &["new", tpl.to_str().unwrap(), ".", "--ref", at, "--force"],
        );
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "template"]);
        for (c, _) in [(&chd, 0), (&chr, 1)] {
            splice(
                &repo,
                &[
                    "channel",
                    "add",
                    &format!("{}//channel", c.display()),
                    "--ref",
                    at,
                ],
            );
            git(&repo, &["add", "-A"]);
            git(&repo, &["commit", "-q", "-m", "channel"]);
        }
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        World { root, repo }
    }

    /// Edit a tracked file and commit it to main (and the origin), as the owner's drift.
    fn edit(&self, rel: &str, f: impl Fn(&str) -> String) {
        let p = self.repo.join(rel);
        let t = std::fs::read_to_string(&p).unwrap();
        std::fs::write(&p, f(&t)).unwrap();
        git(&self.repo, &["add", "-A"]);
        git(&self.repo, &["commit", "-q", "-m", "local edit"]);
        git(&self.repo, &["push", "-q", "origin", "main"]);
    }

    fn keel(&self) -> (i32, serde_json::Value) {
        self.keel_with(&[])
    }

    fn keel_with(&self, extra: &[&str]) -> (i32, serde_json::Value) {
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

    fn branch_log(&self) -> String {
        let b = format!("keel/migrate-v{}", env!("CARGO_PKG_VERSION"));
        git(&self.repo, &["log", "--format=%s", &format!("main..{b}")])
    }
}

fn step<'a>(v: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    v["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap()
}

fn drift_title(v: &serde_json::Value) -> String {
    v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "splice-drift")
        .unwrap_or_else(|| panic!("no splice-drift: {v}"))["title"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn a_drifted_main_file_is_left_not_updated_with_keep_while_the_clean_sources_update() {
    let w = World::new("v1.0.0");
    w.edit("docs/owned.md", |t| format!("{t}mine\n"));
    let before = git(&w.repo, &["status", "--porcelain=v2", "--ignored"]);
    let (code, v) = w.keel_with(&["--drift", "keep"]);
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["status"], "human");
    assert_eq!(step(&v, "splice")["action"], "pending", "{v}");
    // the channels updated, each in its own commit (splice refuses a dirty tree)
    assert_eq!(w.branch_file("docs/agents/rig.md"), "two\n");
    let log = w.branch_log();
    assert!(log.contains("chore: splice update (channel rig)"), "{log}");
    assert!(
        !log.contains("chore: splice update (main template)"),
        "{log}"
    );
    // main untouched
    assert_eq!(w.branch_file("docs/owned.md"), "one\nmine\n");
    let t = drift_title(&v);
    assert!(
        t.contains("NOT updated") && t.contains("main template"),
        "{t}"
    );
    assert!(t.contains("rerun `keel migrate`"), "{t}");
    assert_eq!(w.branch_file("docs/agents/decisions.md"), "two\n");
    assert_eq!(
        before,
        git(&w.repo, &["status", "--porcelain=v2", "--ignored"])
    );
}

#[test]
fn every_source_drifted_is_settled_by_the_default_seed_mode() {
    let w = World::new("v1.0.0");
    w.edit("docs/owned.md", |t| format!("{t}mine\n"));
    w.edit("docs/agents/decisions.md", |t| format!("{t}mine\n"));
    w.edit("docs/agents/rig.md", |t| format!("{t}mine\n"));
    let (code, v) = w.keel();
    assert_eq!(code, 5, "{v}");
    assert_eq!(v["changed"], true);
    let ids: Vec<&str> = v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert!(!ids.contains(&"splice-drift"), "{v}");
    assert_eq!(v["drift"].as_array().unwrap().len(), 3, "{v}");
    assert_eq!(w.branch_file("docs/owned.md"), "two\n");
    assert_eq!(w.branch_file("docs/agents/rig.md"), "one\nmine\n");
    // the local line of the owned file is in the seed file next to it
    assert!(w.branch_file("docs/CLAUDE.md").contains("mine"));
}

#[test]
fn a_section_block_edited_on_an_up_to_date_source_gets_channel_advice() {
    let w = World::new("v2.0.0");
    w.edit("AGENTS.md", |t| t.replace("rules two", "rules MINE"));
    let (code, v) = w.keel();
    assert_eq!(code, 5, "{v}");
    let t = drift_title(&v);
    assert!(
        t.contains("AGENTS.md#decisions") && t.contains("channel decisions"),
        "{t}"
    );
    assert!(t.contains("newer version"), "{t}");
    assert!(!t.contains("--exclude AGENTS.md#decisions"), "{t}");
    assert!(!t.contains("NOT updated"), "{t}");
}

#[test]
fn an_edited_channel_file_on_an_up_to_date_source_is_excluded_by_default() {
    let w = World::new("v2.0.0");
    w.edit("docs/agents/rig.md", |t| format!("{t}mine\n"));
    let (_, v) = w.keel();
    let ids: Vec<&str> = v["checklist"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["id"].as_str())
        .collect();
    assert!(!ids.contains(&"splice-drift"), "{v}");
    assert_eq!(v["drift"][0]["action"], "excluded", "{v}");
    assert_eq!(w.branch_file("docs/agents/rig.md"), "two\nmine\n");
}

#[test]
fn broken_section_markers_on_a_stale_source_are_a_failure_not_old_drift() {
    let w = World::new("v1.0.0");
    w.edit("AGENTS.md", |t| {
        t.replace("splice:decisions:end", "splice:xx:end")
    });
    let (code, v) = w.keel();
    assert_eq!(code, 1, "{v}");
    assert_eq!(step(&v, "splice")["action"], "failed", "{v}");
    assert_eq!(v["branch"], serde_json::Value::Null);
}
