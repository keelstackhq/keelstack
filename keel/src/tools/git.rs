use super::failure;
use crate::exec::{Cmd, Runner};
use std::path::Path;

fn git(r: &dyn Runner, dir: &Path, args: &[&str]) -> crate::exec::Out {
    r.run(&Cmd::new("git", args).dir(dir))
}

pub fn version(r: &dyn Runner) -> Option<String> {
    let o = r.run(&Cmd::new("git", &["--version"]));
    (o.code == 0).then(|| {
        o.stdout
            .trim()
            .trim_start_matches("git version ")
            .to_string()
    })
}

pub fn is_work_tree(r: &dyn Runner, dir: &Path) -> bool {
    git(r, dir, &["rev-parse", "--is-inside-work-tree"]).code == 0
}

pub fn origin(r: &dyn Runner, dir: &Path) -> Option<String> {
    let o = git(r, dir, &["remote", "get-url", "origin"]);
    (o.code == 0).then(|| o.stdout.trim().to_string())
}

/// Does a remote URL (https, ssh or a proxy path) point at `repo` (OWNER/NAME)?
pub fn origin_matches(url: &str, repo: &str) -> bool {
    let u = url.trim().trim_end_matches('/').to_lowercase();
    let u = u.strip_suffix(".git").unwrap_or(&u);
    let repo = repo.to_lowercase();
    u.strip_suffix(&repo)
        .is_some_and(|rest| rest.ends_with('/') || rest.ends_with(':'))
}

pub fn current_branch(r: &dyn Runner, dir: &Path) -> Result<String, String> {
    let c = Cmd::new("git", &["rev-parse", "--abbrev-ref", "HEAD"]).dir(dir);
    let o = r.run(&c);
    if o.code != 0 {
        return Err(failure(&c, &o));
    }
    Ok(o.stdout.trim().to_string())
}

/// §5.1: the branch steps 3-8 work on: `current` unless it is the default branch (or a detached
/// HEAD); then the manifest's `branch`.
pub fn resolve_branch(current: &str, default: &str, branch: &str) -> String {
    if current.is_empty() || current == "HEAD" || current == default {
        branch.to_string()
    } else {
        current.to_string()
    }
}

fn ok(r: &dyn Runner, dir: &Path, args: &[&str]) -> bool {
    git(r, dir, args).code == 0
}

/// Switch to the work branch (§5.1) and return its name. Stays on the current branch unless it is
/// the default branch (or detached). Then `branch`: checked out when it exists locally, tracked
/// from `origin/<branch>` when only the remote has it (after a best-effort fetch), else created.
/// Never returns the default branch: that is an error, so nothing is committed or pushed there.
pub fn ensure_branch(
    r: &dyn Runner,
    dir: &Path,
    default_branch: &str,
    branch: &str,
) -> Result<String, String> {
    let cur = current_branch(r, dir)?;
    let target = resolve_branch(&cur, default_branch, branch);
    if target == default_branch {
        return Err(format!(
            "refusing to commit or push on the default branch {default_branch}; \
             set `branch` in keel.toml to another name"
        ));
    }
    if target == cur {
        return Ok(cur);
    }
    let local = format!("refs/heads/{target}");
    let remote = format!("refs/remotes/origin/{target}");
    let c = if ok(r, dir, &["rev-parse", "--verify", "--quiet", &local]) {
        Cmd::new("git", &["checkout", &target])
    } else {
        // Best effort: offline or a missing remote branch is fine.
        let _ = git(r, dir, &["fetch", "origin", &target]);
        if ok(r, dir, &["rev-parse", "--verify", "--quiet", &remote]) {
            let upstream = format!("origin/{target}");
            Cmd::new("git", &["checkout", "-b", &target, "--track", &upstream])
        } else {
            Cmd::new("git", &["checkout", "-b", &target])
        }
    }
    .dir(dir);
    let o = r.run(&c);
    if o.code != 0 {
        return Err(failure(&c, &o));
    }
    Ok(target)
}

/// Make sure `.gitignore` lists `entry` (e.g. `.keel/`); true when the file changed.
pub fn ensure_ignored(dir: &Path, entry: &str) -> std::io::Result<bool> {
    let p = dir.join(".gitignore");
    let cur = std::fs::read_to_string(&p).unwrap_or_default();
    let bare = entry.trim_end_matches('/');
    if cur
        .lines()
        .any(|l| matches!(l.trim().trim_start_matches('/').trim_end_matches('/'), x if x == bare))
    {
        return Ok(false);
    }
    let mut next = cur;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(entry);
    next.push('\n');
    std::fs::write(&p, next)?;
    Ok(true)
}

/// Commit only `path` when it has uncommitted changes (or is untracked); other staged or
/// modified files are left alone.
pub fn commit_path(r: &dyn Runner, dir: &Path, path: &str, message: &str) -> Result<(), String> {
    commit_paths(r, dir, &[path.to_string()], message)
}

/// Stage and commit exactly `paths`, never anything else: no `git add -A`,
/// no `commit -a`. Nothing to commit (or no path) is fine; callers pass paths that exist.
pub fn commit_paths(
    r: &dyn Runner,
    dir: &Path,
    paths: &[String],
    message: &str,
) -> Result<(), String> {
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    if paths.is_empty() {
        return Ok(());
    }
    let mut st = vec!["status", "--porcelain", "--"];
    st.extend(&paths);
    let st = Cmd::new("git", &st).dir(dir);
    let o = r.run(&st);
    if o.code != 0 {
        return Err(failure(&st, &o));
    }
    if o.stdout.trim().is_empty() {
        return Ok(());
    }
    let mut add = vec!["add", "--"];
    add.extend(&paths);
    let add = Cmd::new("git", &add).dir(dir);
    let o = r.run(&add);
    if o.code != 0 {
        return Err(failure(&add, &o));
    }
    let mut c = vec!["commit", "-m", message, "--"];
    c.extend(&paths);
    let c = Cmd::new("git", &c).dir(dir);
    let o = r.run(&c);
    if o.code != 0 {
        return Err(failure(&c, &o));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::{FakeRunner, Out};

    fn base(head: &str) -> FakeRunner {
        FakeRunner::new().on(&["git", "rev-parse", "--abbrev-ref", "HEAD"], Out::ok(head))
    }

    fn lines(r: &FakeRunner) -> Vec<String> {
        r.calls().iter().map(|c| c.line()).collect()
    }

    #[test]
    fn existing_remote_branch_is_tracked() {
        let r = base("main\n")
            .on(&["git", "rev-parse", "--verify"], Out::with(1, "", ""))
            .on(
                &[
                    "git",
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    "refs/remotes/origin/keel/bootstrap",
                ],
                Out::ok(""),
            )
            .on(&["git", "fetch"], Out::ok(""))
            .on(&["git", "checkout"], Out::ok(""));
        let b = ensure_branch(&r, Path::new("."), "main", "keel/bootstrap").unwrap();
        assert_eq!(b, "keel/bootstrap");
        let l = lines(&r);
        assert!(
            l.contains(&"git fetch origin keel/bootstrap".to_string()),
            "{l:?}"
        );
        assert!(
            l.contains(&"git checkout -b keel/bootstrap --track origin/keel/bootstrap".to_string())
        );
    }

    #[test]
    fn failed_fetch_still_creates_the_branch() {
        let r = base("HEAD\n")
            .on(&["git", "rev-parse", "--verify"], Out::with(1, "", ""))
            .on(&["git", "fetch"], Out::with(128, "", "offline"))
            .on(&["git", "checkout"], Out::ok(""));
        ensure_branch(&r, Path::new("."), "main", "keel/bootstrap").unwrap();
        assert!(lines(&r).contains(&"git checkout -b keel/bootstrap".to_string()));
    }

    #[test]
    fn default_branch_is_never_the_target() {
        let r = base("main\n");
        let e = ensure_branch(&r, Path::new("."), "main", "main").unwrap_err();
        assert!(e.contains("default branch"), "{e}");
        assert!(!lines(&r).iter().any(|l| l.starts_with("git checkout")));
    }

    #[test]
    fn commit_paths_stages_only_the_given_files() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "").unwrap();
        let r = FakeRunner::new()
            .on(&["git", "status"], Out::ok("?? a.txt\n"))
            .on(&["git", "add"], Out::ok(""))
            .on(&["git", "commit"], Out::ok(""));
        commit_paths(&r, d.path(), &["a.txt".into(), "b.txt".into()], "m").unwrap();
        let l = lines(&r);
        assert_eq!(l[1], "git add -- a.txt b.txt");
        assert_eq!(l[2], "git commit -m m -- a.txt b.txt");
        assert!(
            !l.iter().any(|x| x.contains("-A") || x.contains(" -a")),
            "{l:?}"
        );
    }

    #[test]
    fn commit_paths_with_nothing_to_commit_runs_no_add_or_commit() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "").unwrap();
        let r = FakeRunner::new().on(&["git", "status"], Out::ok(""));
        commit_paths(&r, d.path(), &["a.txt".into()], "m").unwrap();
        assert_eq!(lines(&r).len(), 1);
        let r = FakeRunner::new();
        commit_paths(&r, d.path(), &[], "m").unwrap();
        assert!(lines(&r).is_empty());
    }

    #[test]
    fn source_never_adds_everything() {
        let src = include_str!("git.rs");
        let (code, _) = src.split_once("#[cfg(test)]").unwrap();
        assert!(!code.contains("\"-A\"") && !code.contains("\"-a\""));
    }

    #[test]
    fn ensure_ignored_appends_once() {
        let d = tempfile::tempdir().unwrap();
        assert!(ensure_ignored(d.path(), ".keel/").unwrap());
        assert!(!ensure_ignored(d.path(), ".keel/").unwrap());
        std::fs::write(d.path().join(".gitignore"), "target").unwrap();
        assert!(ensure_ignored(d.path(), ".keel/").unwrap());
        assert_eq!(
            std::fs::read_to_string(d.path().join(".gitignore")).unwrap(),
            "target\n.keel/\n"
        );
        std::fs::write(d.path().join(".gitignore"), "/.keel\n").unwrap();
        assert!(!ensure_ignored(d.path(), ".keel/").unwrap());
    }

    #[test]
    fn origin_forms() {
        for u in [
            "https://github.com/Owner/Name.git",
            "git@github.com:owner/name.git",
            "http://local_proxy@127.0.0.1:1234/git/owner/name",
            "https://github.com/owner/name/",
        ] {
            assert!(origin_matches(u, "owner/name"), "{u}");
        }
        assert!(!origin_matches(
            "https://github.com/owner/other-name",
            "owner/name"
        ));
        assert!(!origin_matches(
            "https://github.com/xowner/name",
            "owner/name"
        ));
    }
}
