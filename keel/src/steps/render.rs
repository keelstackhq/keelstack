use super::repo;
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::exec::Cmd;
use crate::step::{Check, Step};
use crate::tools::{self, gh, git, splice};

pub struct Render;

fn template(cx: &Ctx) -> Result<String, KeelError> {
    cx.manifest.template.clone().ok_or_else(|| {
        KeelError::Env(
            "no template: pass --template rust|ts|SOURCE or set `template` in keel.toml".into(),
        )
    })
}

const LIST: &str = ".keel/render-files";

fn save_render_files(dir: &std::path::Path, files: &[String]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join(".keel"))?;
    std::fs::write(dir.join(LIST), files.join("\n") + "\n")
}

/// The render ran but its commit did not (crash in between): commit the saved file list (and the
/// lock file), never `add -A`. No list: commit just the lock and fail naming what is left.
fn resume_uncommitted(cx: &Ctx, tpl: &str) -> Option<Check> {
    let list_path = cx.dir.join(LIST);
    match std::fs::read_to_string(&list_path) {
        Ok(text) => {
            let paths = splice::sanitize_files(text.lines().map(|l| l.trim().to_string()));
            let msg = format!("keel: render {tpl}");
            if let Err(e) = git::commit_paths(cx.runner, &cx.dir, &paths, &msg) {
                return Some(Check::Failed(e));
            }
            let _ = std::fs::remove_file(&list_path);
            None
        }
        Err(_) => {
            let lock = [".splice/lock.toml".to_string()];
            let st = Cmd::new("git", &["status", "--porcelain", "--", &lock[0]]).dir(&cx.dir);
            let o = cx.runner.run(&st);
            if o.code != 0 || o.stdout.trim().is_empty() {
                return None;
            }
            if let Err(e) =
                git::commit_paths(cx.runner, &cx.dir, &lock, &format!("keel: render {tpl}"))
            {
                return Some(Check::Failed(e));
            }
            let all = Cmd::new("git", &["status", "--porcelain"]).dir(&cx.dir);
            let left = cx.runner.run(&all).stdout;
            Some(Check::Failed(format!(
                "a previous render left files uncommitted and keel lost the list (only .splice/lock.toml was committed); `git status` shows:\n{}\nfix: review them and commit the rendered files, then re-run keel up",
                left.trim_end()
            )))
        }
    }
}

impl Step for Render {
    fn id(&self) -> &'static str {
        "render"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let lock = cx.dir.join(".splice/lock.toml").is_file();
        // A saved list means a render whose commit has not happened yet (any template).
        if lock && cx.dir.join(LIST).is_file() {
            return Ok(Check::Todo("a previous render is not committed yet".into()));
        }
        let needs_cargo_lock =
            cx.dir.join("Cargo.toml").is_file() && !cx.dir.join("Cargo.lock").is_file();
        Ok(match (lock, needs_cargo_lock) {
            (true, false) => Check::Done("rendered (.splice/lock.toml)".into()),
            (true, true) => Check::Todo("Cargo.lock is missing".into()),
            (false, _) => Check::Todo(format!("render {}", template(cx)?)),
        })
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let repo = repo(cx)?;
        let tpl = template(cx)?;
        let src = splice::source(&tpl).to_string();
        let r = cx.runner;
        let Some(git_ref) = cx.manifest.template_ref.clone() else {
            return Ok(Check::Failed(format!(
                "no template_ref for {src}: pass --template-ref REF (tag, branch or sha) or set `template_ref` in keel.toml"
            )));
        };
        let lock_before = cx.dir.join(".splice/lock.toml").is_file();

        if !lock_before {
            // The template tag must still point at the commit keel pinned.
            if src.starts_with("gh:keelstackhq/hull") {
                let pinned = cx.pins.ref_sha("hull", &git_ref);
                if let Err(e) = splice::verify_hull_ref(r, &cx.dir, &git_ref, pinned) {
                    return Ok(Check::Failed(e));
                }
            }
            // Validate piece names before touching the checkout.
            match splice::pieces(r, &cx.dir, &src, &git_ref) {
                splice::Pieces::List(list) => {
                    let problems = splice::check_pieces(&cx.manifest.pieces, &list);
                    if !problems.is_empty() {
                        return Ok(Check::Failed(format!("{src}: {}", problems.join("; "))));
                    }
                }
                splice::Pieces::Failed(e) => return Ok(Check::Failed(e)),
                splice::Pieces::Unavailable(w) => eprintln!("keel: warning: {w}"),
            }
        }
        // Every path below commits: be on the work branch first (§5.1).
        let default = match gh::default_branch(r, &repo) {
            Ok(b) => b,
            Err(e) => return Ok(Check::Failed(e)),
        };
        if let Err(e) = git::ensure_branch(r, &cx.dir, &default, cx.manifest.branch_name()) {
            return Ok(Check::Failed(e));
        }
        // The branch may already hold the render (a re-run from a fresh clone).
        let rendered = cx.dir.join(".splice/lock.toml").is_file();
        let mut render_files: Vec<String> = Vec::new();
        if rendered {
            if let Some(failed) = resume_uncommitted(cx, &tpl) {
                return Ok(failed);
            }
        } else {
            // `splice new` refuses a dirty tree: commit keel.toml (only it) on the branch first.
            if let Err(e) = git::commit_path(r, &cx.dir, "keel.toml", "keel: add keel.toml") {
                return Ok(Check::Failed(e));
            }
            let rust = src == splice::RUST_SOURCE;
            let c = splice::new_cmd(&cx.dir, &src, &git_ref, &cx.manifest.pieces, rust);
            let o = r.run(&c);
            if o.code != 0 {
                return Ok(Check::Failed(tools::failure(&c, &o)));
            }
            match tools::json(&o) {
                Some(v) => render_files = splice::render_files(&v),
                None => return Ok(Check::Failed(tools::not_json(&c))),
            }
            // Survive a crash between `splice new` and the commit (see `resume_uncommitted`).
            if let Err(e) = save_render_files(&cx.dir, &render_files) {
                return Ok(Check::Failed(format!("cannot write {LIST}: {e}")));
            }
        }
        let lockfile = cx.dir.join("Cargo.toml").is_file() && !cx.dir.join("Cargo.lock").is_file();
        if lockfile {
            let c = Cmd::new("cargo", &["generate-lockfile"]).dir(&cx.dir);
            let o = r.run(&c);
            if o.code != 0 {
                return Ok(Check::Failed(tools::failure(&c, &o)));
            }
        }
        // keel's scratch dir must never reach the target repo's history.
        let ignored = match git::ensure_ignored(&cx.dir, ".keel/") {
            Ok(c) => c,
            Err(e) => return Ok(Check::Failed(format!("cannot update .gitignore: {e}"))),
        };
        let committed = if rendered {
            // Lockfile-only: commit just what keel changed, never the rest of the worktree.
            let mut res = Ok(());
            if lockfile {
                res = git::commit_path(r, &cx.dir, "Cargo.lock", "keel: add Cargo.lock");
            }
            if res.is_ok() && ignored {
                res = git::commit_path(r, &cx.dir, ".gitignore", "keel: ignore .keel");
            }
            res
        } else {
            // Only what the render wrote, plus what keel itself changed: never `git add -A`.
            let mut paths = render_files;
            if lockfile {
                paths.push("Cargo.lock".into());
            }
            if ignored {
                paths.push(".gitignore".into());
            }
            let res = git::commit_paths(r, &cx.dir, &paths, &format!("keel: render {tpl}"));
            if res.is_ok() {
                let _ = std::fs::remove_file(cx.dir.join(LIST));
            }
            res
        };
        if let Err(e) = committed {
            return Ok(Check::Failed(e));
        }
        self.check(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::{FakeRunner, Out};

    fn fake() -> FakeRunner {
        FakeRunner::new()
            .on(
                &["gh", "repo", "view"],
                Out::ok(r#"{"defaultBranchRef":{"name":"main"}}"#),
            )
            .on(
                &["git", "rev-parse", "--abbrev-ref", "HEAD"],
                Out::ok("main\n"),
            )
            .on(&["git", "rev-parse", "--verify"], Out::with(1, "", ""))
            .on(&["git", "checkout", "-b", "keel/bootstrap"], Out::ok(""))
            .on(
                &["git", "ls-remote"],
                Out::ok("3f4a65fda568cdd20c76168ed156d25a28ba5630\trefs/tags/v0.13.0\n"),
            )
            .on(
                &["splice", "--json", "pieces"],
                Out::ok(r#"{"ok":true,"command":"pieces","pieces":[{"id":"rust-cli","multiple":true,"default_name":null},{"id":"docs"}]}"#),
            )
            .on(
                &["splice", "--json", "--yes", "new"],
                Out::ok(
                    r#"{"ok":true,"changes":[{"path":"src/main.rs","action":"create"},{"path":"skipped.txt","action":"skip"}]}"#,
                ),
            )
            .on(&["cargo", "generate-lockfile"], Out::ok(""))
            .on(&["git", "status", "--porcelain"], Out::ok("?? keel.toml\n"))
            .on(&["git", "add"], Out::ok(""))
            .on(&["git", "commit"], Out::ok(""))
    }

    fn apply(r: &FakeRunner, dir: &std::path::Path, tpl: &str) -> Check {
        let mut cx = ctx(
            r,
            dir.to_path_buf(),
            manifest(tpl),
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        Render.apply(&mut cx).unwrap()
    }

    fn lines(r: &FakeRunner) -> Vec<String> {
        r.calls().iter().map(|c| c.line()).collect()
    }

    #[test]
    fn check_states() {
        let d = tempfile::tempdir().unwrap();
        let r = FakeRunner::new();
        let mut cx = ctx(
            &r,
            d.path().into(),
            manifest("rust"),
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        assert!(matches!(Render.check(&mut cx).unwrap(), Check::Todo(_)));
        std::fs::create_dir(d.path().join(".splice")).unwrap();
        std::fs::write(d.path().join(".splice/lock.toml"), "").unwrap();
        assert!(matches!(Render.check(&mut cx).unwrap(), Check::Done(_)));
        std::fs::write(d.path().join("Cargo.toml"), "").unwrap();
        assert!(
            matches!(Render.check(&mut cx).unwrap(), Check::Todo(m) if m.contains("Cargo.lock"))
        );
    }

    #[test]
    fn render_builds_exact_argv_in_order() {
        let d = tempfile::tempdir().unwrap();
        let r = fake();
        // Not done after apply (the fake writes no lock file): only the calls matter here.
        let _ = apply(&r, d.path(), "rust");
        let l = lines(&r);
        let pos = |s: &str| {
            l.iter()
                .position(|x| x == s)
                .unwrap_or_else(|| panic!("{s} in {l:?}"))
        };
        let new = "splice --json --yes new gh:keelstackhq/hull//template-rust . --ref v0.13.0 --force --piece rust-cli:my-cli --data no_placeholder=true";
        let kc = "git commit -m keel: add keel.toml -- keel.toml";
        assert!(pos("git checkout -b keel/bootstrap") < pos("git add -- keel.toml"));
        assert!(pos("git add -- keel.toml") < pos(kc));
        assert!(pos(kc) < pos(new));
        let add = "git add -- .splice/lock.toml src/main.rs .gitignore";
        assert!(pos(new) < pos(add));
        assert!(
            pos(add)
                < pos(
                    "git commit -m keel: render rust -- .splice/lock.toml src/main.rs .gitignore"
                )
        );
        assert!(!l.iter().any(|x| x.contains("skipped.txt")), "{l:?}");
        assert!(
            !l.iter()
                .any(|x| x.contains(" -A") || x.contains("commit -a")),
            "{l:?}"
        );
        assert_eq!(
            r.calls()
                .iter()
                .find(|c| c.program == "splice" && c.args[2] == "new")
                .unwrap()
                .dir
                .as_deref(),
            Some(d.path())
        );
    }

    #[test]
    fn clean_keel_toml_is_not_recommitted() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(&["git", "status", "--porcelain"], Out::ok(""));
        let _ = apply(&r, d.path(), "rust");
        let l = lines(&r);
        assert!(
            !l.iter().any(|x| x.contains("keel: add keel.toml")),
            "{l:?}"
        );
        assert!(l.iter().any(|x| x.starts_with("splice --json --yes new")));
    }

    #[test]
    fn ts_template_has_no_rust_data_flag() {
        let d = tempfile::tempdir().unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "ts");
        assert!(
            lines(&r).contains(
                &"splice --json --yes new gh:keelstackhq/hull//template . --ref v0.13.0 --force --piece rust-cli:my-cli"
                    .to_string()
            )
        );
    }

    #[test]
    fn current_feature_branch_is_kept() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(
            &["git", "rev-parse", "--abbrev-ref", "HEAD"],
            Out::ok("claude/work\n"),
        );
        let _ = apply(&r, d.path(), "rust");
        assert!(!lines(&r).iter().any(|l| l.starts_with("git checkout")));
    }

    #[test]
    fn pieces_exit_2_warns_and_renders() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(
            &["splice", "--json", "pieces"],
            Out::with(2, "", "unknown command"),
        );
        let _ = apply(&r, d.path(), "rust");
        assert!(
            lines(&r)
                .iter()
                .any(|l| l.starts_with("splice --json --yes new"))
        );
    }

    #[test]
    fn unknown_piece_fails_listing_valid_ids_before_any_write() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(
            &["splice", "--json", "pieces"],
            Out::ok(r#"{"ok":true,"command":"pieces","pieces":[{"id":"docs"},{"id":"web"}]}"#),
        );
        let Check::Failed(m) = apply(&r, d.path(), "rust") else {
            panic!()
        };
        assert!(m.contains("rust-cli") && m.contains("docs, web"), "{m}");
        assert!(!lines(&r).iter().any(|l| (l.starts_with("git")
            && !l.starts_with("git ls-remote"))
            || l.contains(" new ")));
    }

    #[test]
    fn custom_source_without_a_template_ref_fails_asking_for_one() {
        let d = tempfile::tempdir().unwrap();
        let r = fake();
        let mut m = manifest("gh:x/y");
        m.template_ref = None;
        let mut cx = ctx(
            &r,
            d.path().to_path_buf(),
            m,
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        let Check::Failed(e) = Render.apply(&mut cx).unwrap() else {
            panic!()
        };
        assert!(
            e.contains("--template-ref") && e.contains("template_ref"),
            "{e}"
        );
        assert!(lines(&r).is_empty(), "nothing runs before the ref is known");
    }

    fn pinned(d: &std::path::Path, r: &FakeRunner, tag: &str) -> Check {
        let mut m = manifest("rust");
        m.template_ref = Some(tag.into());
        let mut cx = ctx(
            r,
            d.to_path_buf(),
            m,
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        Render.apply(&mut cx).unwrap()
    }

    #[test]
    fn moved_or_unpinned_hull_tag_fails_before_splice() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(&["git", "ls-remote"], Out::ok("bad\trefs/tags/v0.13.0\n"));
        let Check::Failed(m) = pinned(d.path(), &r, "v0.13.0") else {
            panic!()
        };
        assert!(m.contains("moved") && m.contains("bad"), "{m}");
        let r = fake();
        let Check::Failed(m) = pinned(d.path(), &r, "v0.99.0") else {
            panic!()
        };
        assert!(m.contains("KEEL_PINS"), "{m}");
        let r = fake().on(&["git", "ls-remote"], Out::with(128, "", "no net"));
        assert!(matches!(pinned(d.path(), &r, "v0.13.0"), Check::Failed(_)));
        assert!(!lines(&r).iter().any(|l| l.contains(" new ")));
    }

    #[test]
    fn render_file_list_is_saved_and_removed_after_the_commit() {
        let d = tempfile::tempdir().unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "rust");
        assert!(!d.path().join(LIST).exists());
        // A failing commit leaves the list for the next run.
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(
            &["git", "commit", "-m", "keel: render rust"],
            Out::with(1, "", "x"),
        );
        let _ = apply(&r, d.path(), "rust");
        let t = std::fs::read_to_string(d.path().join(LIST)).unwrap();
        assert_eq!(t, ".splice/lock.toml\nsrc/main.rs\n");
    }

    fn lock(d: &std::path::Path) {
        std::fs::create_dir(d.join(".splice")).unwrap();
        std::fs::write(d.join(".splice/lock.toml"), "").unwrap();
    }

    fn check(r: &FakeRunner, dir: &std::path::Path, tpl: &str) -> Check {
        let mut cx = ctx(
            r,
            dir.to_path_buf(),
            manifest(tpl),
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        Render.check(&mut cx).unwrap()
    }

    #[test]
    fn a_failed_render_commit_resumes_even_when_cargo_lock_exists() {
        for tpl in ["rust", "ts"] {
            let d = tempfile::tempdir().unwrap();
            // First run: splice renders, cargo writes Cargo.lock, the render commit fails.
            let r = fake().on(
                &["git", "commit", "-m", &format!("keel: render {tpl}")],
                Out::with(1, "", "commit failed"),
            );
            assert!(
                matches!(apply(&r, d.path(), tpl), Check::Failed(_)),
                "{tpl}"
            );
            assert!(d.path().join(LIST).is_file(), "{tpl}: the list survives");
            // What splice and cargo wrote on disk.
            lock(d.path());
            if tpl == "rust" {
                std::fs::write(d.path().join("Cargo.toml"), "").unwrap();
                std::fs::write(d.path().join("Cargo.lock"), "").unwrap();
            }
            // Re-run: not done until the render is committed.
            assert!(
                matches!(check(&fake(), d.path(), tpl), Check::Todo(_)),
                "{tpl}"
            );
            let r = fake();
            let _ = apply(&r, d.path(), tpl);
            let l = lines(&r);
            assert!(
                l.iter().any(
                    |x| x.starts_with(&format!("git commit -m keel: render {tpl} --"))
                        && x.contains("src/main.rs")
                ),
                "{tpl}: {l:?}"
            );
            assert!(!l.iter().any(|x| x.starts_with("splice")), "{tpl}: {l:?}");
            assert!(!d.path().join(LIST).exists(), "{tpl}");
            assert!(
                matches!(check(&fake(), d.path(), tpl), Check::Done(_)),
                "{tpl}"
            );
        }
    }

    #[test]
    fn resume_commits_the_saved_list_filtered_and_removes_it() {
        let d = tempfile::tempdir().unwrap();
        lock(d.path());
        std::fs::create_dir(d.path().join(".keel")).unwrap();
        std::fs::write(
            d.path().join(LIST),
            "src/main.rs\n../evil\n/etc/x\n.git/x\n",
        )
        .unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "rust");
        let l = lines(&r);
        assert!(
            l.contains(
                &"git commit -m keel: render rust -- .splice/lock.toml src/main.rs".to_string()
            ),
            "{l:?}"
        );
        assert!(
            !l.iter().any(|x| x.contains("evil") || x.contains(" -A")),
            "{l:?}"
        );
        assert!(!l.iter().any(|x| x.starts_with("splice")), "{l:?}");
        assert!(!d.path().join(LIST).exists());
    }

    #[test]
    fn resume_without_a_list_commits_only_the_lock_and_fails_with_a_fix() {
        let d = tempfile::tempdir().unwrap();
        lock(d.path());
        let r = fake();
        let Check::Failed(m) = apply(&r, d.path(), "rust") else {
            panic!()
        };
        assert!(m.contains("git status") && m.contains("commit"), "{m}");
        assert!(
            lines(&r).contains(&"git commit -m keel: render rust -- .splice/lock.toml".to_string())
        );
        // Clean lock and no list: nothing to resume.
        let d = tempfile::tempdir().unwrap();
        lock(d.path());
        let r = fake().on(&["git", "status", "--porcelain"], Out::ok(""));
        let _ = apply(&r, d.path(), "rust");
        assert!(!lines(&r).iter().any(|l| l.starts_with("git commit")));
    }

    #[test]
    fn splice_conflict_fails() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(
            &["splice", "--json", "--yes", "new"],
            Out::with(1, "", "conflict"),
        );
        assert!(
            matches!(apply(&r, d.path(), "rust"), Check::Failed(m) if m.contains("splice --json --yes new"))
        );
    }

    #[test]
    fn cargo_lock_generated_when_missing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(".splice")).unwrap();
        std::fs::write(d.path().join(".splice/lock.toml"), "").unwrap();
        std::fs::write(d.path().join("Cargo.toml"), "").unwrap();
        std::fs::create_dir(d.path().join(".keel")).unwrap();
        std::fs::write(d.path().join(LIST), ".splice/lock.toml\n").unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "rust");
        let l = lines(&r);
        assert!(l.contains(&"cargo generate-lockfile".to_string()));
        assert!(!l.iter().any(|x| x.starts_with("splice")), "{l:?}");
        // On the work branch, committing only Cargo.lock (and the .keel ignore).
        let pos = |p: &str| {
            l.iter()
                .position(|x| x == p)
                .unwrap_or_else(|| panic!("{p} in {l:?}"))
        };
        assert!(pos("git checkout -b keel/bootstrap") < pos("cargo generate-lockfile"));
        assert!(l.contains(&"git add -- Cargo.lock".to_string()));
        assert!(!l.iter().any(|x| x.starts_with("git add -A")), "{l:?}");
    }

    #[test]
    fn keel_dir_is_gitignored_and_committed_with_the_render() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join(".gitignore"), "target\n").unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "rust");
        let g = std::fs::read_to_string(d.path().join(".gitignore")).unwrap();
        assert!(g.lines().any(|l| l == ".keel/"), "{g}");
        let l = lines(&r);
        let p = |s: &str| l.iter().position(|x| x == s).unwrap();
        assert!(
            p(
                "splice --json --yes new gh:keelstackhq/hull//template-rust . --ref v0.13.0 --force --piece rust-cli:my-cli --data no_placeholder=true"
            ) < p("git add -- .splice/lock.toml src/main.rs .gitignore")
        );
    }

    #[test]
    fn full_source_of_the_rust_template_gets_the_rust_flags() {
        let d = tempfile::tempdir().unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "gh:keelstackhq/hull//template-rust");
        assert!(
            lines(&r)
                .iter()
                .any(|l| l.starts_with("splice --json --yes new")
                    && l.ends_with("--data no_placeholder=true"))
        );
    }
}
