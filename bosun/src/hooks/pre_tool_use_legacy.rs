//! The legacy hook: main's (d90f917) `bosun hook pre-tool-use` judge, vendored verbatim with
//! main's guard ([`crate::core::guard::legacy`]) and main's script reader, so the legacy judge sees
//! exactly the input main saw. The live hook ([`super::pre_tool_use`]) runs it first and returns
//! its deny at once (the no-regression rule). Three differences from main, none of which judges a
//! call: no repeat-lock (the live hook keeps its count; running it twice would double it), no
//! stale-verdict notes and no repeat-file reset (side effects the live hook performs once).
//! Returns the hook's deny output or `None`; never an allow note.

use std::path::Path;

use serde_json::Value;

use super::{cache, git_out};
use crate::app::{App, trusted_notes};
use crate::cli::stamps;
use crate::config::Config;
use crate::core::guard::legacy::{
    ChildTask, Script, answers_lock, bash_writes, bosun_dir_lock, child_task_create,
    config_fetch_risk, edit_lock, edited_path, foreign_lock, prereqs_lock, prereqs_lock_for,
    pushes, script_rules, settings_rule, subagent_lock, tool_rules,
};
use crate::core::lease::leases_of;
use crate::core::mode::PushGates;
use crate::core::stamps::gate_loop;
use crate::core::touch::{is_plan_file, is_plan_tick};
use crate::ctx::own_session;

use super::pre_tool_use::deny;

/// Main's judge (see the module doc for the three omissions).
pub fn judge(cwd: Option<&Path>, input: &Value) -> Option<String> {
    let tool = input.get("tool_name").and_then(Value::as_str)?;
    let tool_input = input.get("tool_input").cloned().unwrap_or(Value::Null);
    let dir = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())?;
    // The branch where a git command acts: the hook's directory moved by the command's own
    // `cd dir &&` and `git -C dir` (a worktree on a feature branch is not the default branch).
    let branch_at = |opts: &[String]| {
        let mut args: Vec<&str> = opts.iter().map(String::as_str).collect();
        args.extend(["rev-parse", "--abbrev-ref", "HEAD"]);
        git_out(&dir, &args)
    };
    let default = git_out(
        &dir,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .map(|b| b.strip_prefix("origin/").unwrap_or(&b).to_string())
    .unwrap_or_else(|| "main".into());
    // A subagent shares its parent's session and environment: it never merges or signs.
    let agent = input
        .get("agent_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !agent.trim().is_empty()
        && let Some(why) = subagent_lock(tool, &tool_input)
    {
        return Some(deny(&why));
    }
    // The settings-lock starts in the hook's own directory (which may be inside `.claude`).
    if let Some(why) = settings_rule(tool, &tool_input, &dir.to_string_lossy()) {
        return Some(deny(&why));
    }
    if let Some(why) = tool_rules(tool, &tool_input, &branch_at, &default, &own_session()) {
        // `own_session` is the ctx session id (the hook's input `session_id` is Claude's UUID).
        return Some(deny(&why));
    }
    if let Some(why) = answers_lock(tool, &tool_input).or_else(|| bosun_dir_lock(tool, &tool_input))
    {
        return Some(deny(&why));
    }
    if let Some(why) = prereqs_lock(tool, &tool_input) {
        // A registered child may create a new issue with a task marker for its own epic, and
        // nothing else: not edits, comments, other markers, other repos or the allow and
        // capabilities keys.
        let child_ok = prereqs_lock_for(tool, &tool_input, true).is_none()
            && child_task_create(tool, &tool_input)
                .is_ok_and(|write| registered_child(&dir, &write));
        if !child_ok {
            return Some(deny(&why));
        }
    }
    if tool == "Bash" {
        // Edit-lock for the files a command plainly writes (`> f`, `tee f`, `cp .. f`, `sed -i`),
        // then repeat-lock; script files are read last, after every other lock.
        let command = tool_input
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("");
        for path in bash_writes(command) {
            match edit_reason(&dir, &path) {
                Ok(Some(why)) => return Some(deny(&why)),
                Ok(None) => {}
                Err(e) => eprintln!("bosun: warning: edit-lock skipped: {e}"),
            }
        }
        let read = |p: &str| read_script(&dir, p);
        if let Some(why) = script_rules(command, &read, &branch_at, &default) {
            return Some(deny(&why));
        }
        if let Some(why) = push_config(&dir, command, &read, &default) {
            return Some(deny(&why));
        }
        if let Some(why) = fetch_config(&dir, command, &default) {
            return Some(deny(&why));
        }
        let pushed = match push_gates(&dir, command, &read, &default) {
            Ok(p) => p,
            Err(why) => return Some(deny(&why)),
        };
        let _ = pushed;
        return None;
    }
    let path = edited_path(tool, &tool_input)?;
    if plan_tick_in(&dir, tool, &path, &tool_input) {
        return None; // a checkbox tick in a plan needs no lease (spec §4)
    }
    match edit_reason(&dir, &path) {
        Ok(why) => why.map(|w| deny(&w)),
        Err(e) => {
            eprintln!("bosun: warning: edit-lock skipped: {e}");
            None
        }
    }
}

/// Merge-lock (spec §4) for pushes whose target git config sets, read where git runs: any
/// `remote.<r>.push` or `remote.<r>.mirror` refuses every push; without a refspec,
/// `push.default` must push `HEAD` to its own name or to an upstream that is not the default
/// branch. A checkout the guard cannot tell is read at the hook directory.
fn push_config(
    dir: &Path,
    command: &str,
    read: &dyn Fn(&str) -> Option<Script>,
    default: &str,
) -> Option<String> {
    const FIX: &str = "name the refspec, as in `git push -u origin claude/fix-1`";
    const UNREAD: &str =
        "merge-lock: the guard cannot read git config here, so it cannot tell where this push goes";
    if let Some(why) = alias_push(dir, command) {
        return Some(why);
    }
    for push in pushes(command, read, default) {
        let opts = push.dir.unwrap_or_default();
        let at = |args: &[&str]| {
            let mut all: Vec<&str> = opts.iter().map(String::as_str).collect();
            all.extend(args);
            git_out(dir, &all)
        };
        // Exit 1 is "no such key"; any other failure (not a repo, dubious ownership) fails closed.
        let get = |args: &[&str]| -> Result<Option<String>, ()> {
            let mut all: Vec<&str> = opts.iter().map(String::as_str).collect();
            all.extend(args);
            config_get(dir, &all)
        };
        let Ok(set) = get(&["config", "--get-regexp", r"^remote\..*\.(push|mirror)$"]) else {
            return Some(UNREAD.into());
        };
        for line in set.iter().flat_map(|s| s.lines()) {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            if key.ends_with(".push") || !value.eq_ignore_ascii_case("false") {
                return Some(format!(
                    "merge-lock: git config `{key} {value}` sets where pushes go; unset it \
                     (`git config --unset-all {key}`) or ask the owner"
                ));
            }
        }
        if !push.srcs.is_empty() {
            continue;
        }
        let Ok(mode) = get(&["config", "--get", "push.default"]) else {
            return Some(UNREAD.into());
        };
        let mode = mode.map(|m| m.to_ascii_lowercase());
        match mode.as_deref() {
            None | Some("simple" | "current" | "nothing") => {}
            Some("upstream" | "tracking") => {
                let up = at(&["rev-parse", "--abbrev-ref", "@{upstream}"]);
                let branch = up.as_deref().and_then(|u| u.split_once('/')).map(|p| p.1);
                if branch.is_none_or(|b| b == default || b.ends_with(&format!("/{default}"))) {
                    return Some(format!(
                        "merge-lock: push.default = upstream pushes to `{}`, which the guard \
                         cannot tell apart from `{default}`; {FIX}",
                        up.as_deref().unwrap_or("no upstream")
                    ));
                }
            }
            Some(m) => {
                return Some(format!(
                    "merge-lock: push.default = {m} can push `{default}`; {FIX}"
                ));
            }
        }
    }
    None
}

/// Merge-lock (spec §4) for a fetch or pull: the `remote.<r>.fetch` refspecs it follows and the
/// protocol and URL rewrites that run a command, asked of git where it runs (each earlier
/// `cd`, then `git -C`). Git that cannot be asked (a missing directory) fails closed.
fn fetch_config(dir: &Path, command: &str, default: &str) -> Option<String> {
    const KEYS: &str = r"^(remote\..*\.(fetch|url|pushurl|uploadpack|receivepack|tagopt)|core\.sshcommand|protocol\..*allow|url\..*insteadof)$";
    let unq = |w: &str| w.replace(['\'', '"', '\\'], "");
    let base = |w: &str| unq(w).rsplit('/').next().unwrap_or("").to_string();
    let mut at: Vec<String> = Vec::new();
    for seg in command.split([';', '&', '|', '\n']) {
        let w: Vec<String> = seg.split_whitespace().map(unq).collect();
        if w.first().is_some_and(|c| c == "cd" || c == "pushd") {
            at.extend(["-C".to_string(), w.get(1).cloned().unwrap_or_default()]);
            continue;
        }
        // git's global options, in any order and attached or not, move where it reads config.
        let (mut opts, mut i) = (at.clone(), 1);
        while let Some(a) = w.get(i).filter(|a| a.starts_with('-')) {
            let pair = [
                "-C",
                "-c",
                "--git-dir",
                "--work-tree",
                "--namespace",
                "--config-env",
            ];
            let n = 1 + usize::from(pair.contains(&a.as_str()));
            opts.extend(w[i..(i + n).min(w.len())].iter().cloned());
            i += n;
        }
        let sub: Vec<&str> = w.iter().skip(i).map(String::as_str).collect();
        // `git remote update` fetches every remote without a fetch word.
        let fetches = sub.first() == Some(&"remote") && sub.contains(&"update")
            || w.iter()
                .any(|a| matches!(base(a).trim_start_matches("git-"), "fetch" | "pull"));
        if !fetches {
            continue;
        }
        let mut all: Vec<&str> = opts.iter().map(String::as_str).collect();
        all.extend(["config", "--null", "--get-regexp", KEYS]);
        let Ok(set) = config_get(dir, &all) else {
            return Some(
                "merge-lock: the guard cannot read git config where this fetch runs".into(),
            );
        };
        for entry in set.iter().flat_map(|s| s.split('\0')) {
            let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
            if config_fetch_risk(key, value, default) {
                return Some(format!(
                    "merge-lock: git config `{key} {value}` makes a fetch move `{default}`, \
                     mirror, write tags or run a command; unset it (`git config --unset-all \
                     {key}`) or ask the owner"
                ));
            }
        }
    }
    None
}

/// `git config` read: `Ok(None)` when the key is unset (exit 1), `Err` when git fails.
fn config_get(dir: &Path, args: &[&str]) -> Result<Option<String>, ()> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|_| ())?;
    match out.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&out.stdout).trim().to_string(),
        )),
        Some(1) => Ok(None),
        _ => Err(()),
    }
}

/// A git alias that may push, run in git's subcommand position (after its global options) and
/// read where that git may run (the hook directory, moved by each earlier literal `cd` or
/// `pushd` and by `-C`, `--git-dir`, `--work-tree`):
/// a value with a `push`, `send-pack` or `subtree` word, or one that starts with `!` (a shell)
/// or `-` (global options). The merge-lock cannot see behind it.
fn alias_push(dir: &Path, command: &str) -> Option<String> {
    let unq = |w: &str| w.replace(['\'', '"', '\\'], "");
    let spaced: String = command
        .chars()
        .flat_map(|c| match c {
            ';' | '&' | '|' | '\n' | '(' | ')' => vec![' ', c, ' '],
            _ => vec![c],
        })
        .collect();
    let words: Vec<String> = spaced.split_whitespace().map(unq).collect();
    // Every directory an earlier `cd` or `pushd` may have moved to (as `-C` options); an alias
    // is read in each. A target the guard cannot read leaves only git's builtins.
    // `CDPATH` (set in the command or in the environment the shell inherits, as the hook's
    // own) makes `cd x` land somewhere other than `./x`.
    let cdpath = words.iter().any(|w| w.starts_with("CDPATH="))
        || std::env::var_os("CDPATH").is_some_and(|v| !v.is_empty());
    let (mut dirs, mut known) = (vec![Vec::<String>::new()], !cdpath);
    let mut i = 0;
    while i < words.len() {
        let w = words[i].clone();
        i += 1;
        if matches!(w.as_str(), "cd" | "pushd") {
            match words.get(i).filter(|t| {
                !t.is_empty()
                    && !t.starts_with(['-', '~'])
                    && !t.contains(['$', '`', '*', '?', '{', ';', '&', '|', '(', ')'])
            }) {
                Some(t) => {
                    let mut d = dirs.last().cloned().unwrap_or_default();
                    d.extend(["-C".to_string(), t.clone()]);
                    dirs.push(d);
                }
                None => known = false,
            }
            continue;
        }
        if w.rsplit('/').next() != Some("git") {
            continue;
        }
        let mut opts: Vec<String> = Vec::new();
        while let Some(o) = words.get(i).filter(|o| o.starts_with('-')) {
            let next = words.get(i + 1).cloned().unwrap_or_default();
            i += 1;
            match o.as_str() {
                "-C" => {
                    opts.extend(["-C".into(), next]);
                    i += 1;
                }
                "--git-dir" | "--work-tree" => {
                    opts.push(format!("{o}={next}"));
                    i += 1;
                }
                "-c" | "--config-env" | "--namespace" => i += 1,
                _ if o.starts_with("--git-dir=") || o.starts_with("--work-tree=") => {
                    opts.push(o.clone())
                }
                _ => {}
            }
        }
        let Some(sub) = words.get(i) else { break };
        let key = format!("alias.{sub}");
        // A directory git cannot open from here (a `cd` it reads differently: CDPATH, a quoted
        // path with spaces) is unknown as well.
        let mut unknown = !known;
        for d in &dirs {
            let mut args: Vec<&str> = d.iter().chain(&opts).map(String::as_str).collect();
            args.extend(["config", "--get", &key]);
            let value = match config_get(dir, &args) {
                Ok(Some(v)) => v,
                Ok(None) => continue,
                Err(()) => {
                    unknown = true;
                    continue;
                }
            };
            let v = value.trim_start();
            let risky = v.starts_with(['!', '-'])
                || v.split_whitespace()
                    .map(unq)
                    .any(|w| matches!(w.as_str(), "push" | "send-pack" | "subtree"));
            if risky {
                return Some(format!(
                    "merge-lock: git alias `{sub}` runs `{value}`, which the guard cannot judge; run the git command itself"
                ));
            }
        }
        if unknown {
            let Some(builtins) = git_out(dir, &["--list-cmds=builtins,main"]) else {
                return Some(
                    "merge-lock: the guard cannot read git config or git's commands here".into(),
                );
            };
            if !builtins.lines().any(|b| b == sub) {
                return Some(format!(
                    "merge-lock: `git {sub}` after a `cd` the guard cannot read may be an alias; \
                     `cd` to a literal path, or run the git command itself"
                ));
            }
        }
    }
    None
}

/// `push_gates = "push"` (spec §7): each pushed ref's tree needs a passing stamp for every
/// ready gate, read in the checkout where git runs. A repo whose config is unset or `ready`
/// pushes as before; a push whose checkout the guard cannot tell (interpreter code, a `cd`
/// it cannot read, `--git-dir`) is refused when the hook directory's repo is in push mode.
/// `Err` is the deny reason; `Ok` lists each ref pushed in push mode, for [`stale_verdicts`].
fn push_gates(
    dir: &Path,
    command: &str,
    read: &dyn Fn(&str) -> Option<Script>,
    default: &str,
) -> Result<Vec<Pushed>, String> {
    let mut pushed = Vec::new();
    for mut push in pushes(command, read, default) {
        // `--git-dir` and `--work-tree` name a checkout `--show-toplevel` does not report.
        push.dir = push.dir.filter(|d| d.iter().all(|o| !o.starts_with("--")));
        let opts = push.dir.clone().unwrap_or_default();
        let at = |args: &[&str]| {
            let mut all: Vec<&str> = opts.iter().map(String::as_str).collect();
            all.extend(args);
            git_out(dir, &all)
        };
        let Some(root) = at(&["rev-parse", "--show-toplevel"]) else {
            continue;
        };
        let root = Path::new(&root);
        let push_mode = Config::load(root)
            .is_ok_and(|c| c.mode_effective().knobs.push_gates.value == PushGates::Push);
        if !push_mode {
            continue;
        }
        if push.dir.is_none() {
            return Err(
                "push-gates: the guard cannot tell which checkout this push runs in; push with \
                 a literal `cd <dir> &&` or `git -C <dir>`"
                    .into(),
            );
        }
        let gates = match stamps::ready_gates(root) {
            Ok(g) => g,
            Err(e) => return Err(format!("push-gates: {}; {}", e.message, e.fix)),
        };
        let srcs = if push.srcs.is_empty() {
            vec!["HEAD".to_string()]
        } else {
            push.srcs
        };
        for src in srcs {
            let Some(tree) = at(&["rev-parse", "--verify", "-q", &format!("{src}^{{tree}}")])
            else {
                return Err(format!(
                    "push-gates: cannot read the tree of `{src}`; name a local branch to push"
                ));
            };
            let missing: Vec<String> = gates
                .iter()
                .filter(|g| !stamps::has(root, &tree, g))
                .cloned()
                .collect();
            if !missing.is_empty() && !stamps::carried(root, &src, &gates, &at) {
                return Err(format!(
                    "push-gates (push_gates = \"push\"): the tree of `{src}` has no passing stamp \
                     for {}: commit your work, run `{}`, fix what fails, then push again",
                    missing.join(", "),
                    gate_loop(&missing)
                ));
            }
            let sha = at(&["rev-parse", "--verify", "-q", &format!("{src}^{{commit}}")]);
            let branch = at(&["rev-parse", "--abbrev-ref", &src]);
            if let (Some(sha), Some(branch)) = (sha, branch)
                && branch != "HEAD"
                && branch != default
            {
                pushed.push(Pushed {
                    root: root.to_path_buf(),
                    sha,
                    branch,
                });
            }
        }
    }
    Ok(pushed)
}

/// A ref that push-gates let through: its checkout, commit and branch.
#[allow(dead_code)] // main read these for its stale-verdict notes
struct Pushed {
    root: std::path::PathBuf,
    sha: String,
    branch: String,
}

/// A script the merge-lock scans, relative to `dir`: at most 1 MiB is read (procfs files report
/// size 0, so the size is not trusted), as lossy UTF-8. A directory or a missing path is
/// `None`; a device, FIFO, socket, a file under /proc, /sys or /dev, or more than 1 MiB is
/// `Unreadable`. A denial may quote a script line, as for any command.
fn read_script(dir: &Path, path: &str) -> Option<Script> {
    use std::io::Read;
    const MAX: u64 = 1 << 20;
    let file = dir.join(path);
    let meta = std::fs::metadata(&file).ok()?;
    if meta.is_dir() {
        return None;
    }
    let real = std::fs::canonicalize(&file).unwrap_or(file.clone());
    if ["/proc", "/sys", "/dev"]
        .iter()
        .any(|r| real.starts_with(r))
    {
        return Some(Script::Unreadable("a /proc, /sys or /dev file"));
    }
    if !meta.is_file() {
        return Some(Script::Unreadable("not a regular file"));
    }
    // Open without blocking (a path swapped for a FIFO after the check must not hang the
    // hook), then judge the opened file itself.
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
        const O_NONBLOCK: i32 = 0x0004;
        #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "freebsd")))]
        const O_NONBLOCK: i32 = 0o4000;
        options.custom_flags(O_NONBLOCK);
    }
    let opened = options.open(&file);
    if opened
        .as_ref()
        .is_ok_and(|f| !f.metadata().is_ok_and(|m| m.is_file()))
    {
        return Some(Script::Unreadable("not a regular file"));
    }
    let mut buf = Vec::new();
    let read = opened.and_then(|f| f.take(MAX + 1).read_to_end(&mut buf));
    match read {
        Err(_) => Some(Script::Unreadable("it cannot be opened")),
        Ok(n) if n as u64 > MAX => Some(Script::Unreadable("larger than 1 MiB")),
        Ok(_) => Some(Script::Text(String::from_utf8_lossy(&buf).into_owned())),
    }
}

/// Whether this session is a registered child for `write.epic`: the epic carries a live lease
/// for this session with origin `spawned` and an empty touch, which only `bosun spawn --record`
/// writes (a claim has touch paths), in a comment of a repo writer (`trusted_notes`, not any
/// comment). An MCP call must also target this repo. Any doubt (no repo, no GitHub) is no.
fn registered_child(dir: &Path, write: &ChildTask) -> bool {
    if super::session_start::unadopted(dir) {
        return false; // no registry here: no GitHub API call
    }
    let Ok(app) = App::load(Some(dir)) else {
        return false;
    };
    if let Some((owner, name)) = &write.repo
        && !(owner.eq_ignore_ascii_case(&app.ctx.repo.owner)
            && name.eq_ignore_ascii_case(&app.ctx.repo.name))
    {
        return false;
    }
    let Ok(notes) = trusted_notes(app.gh.as_ref(), write.epic) else {
        return false;
    };
    leases_of(write.epic, &notes).iter().any(|l| {
        l.session == app.ctx.session
            && l.origin == "spawned"
            && l.touch.is_empty()
            && l.is_live(app.ctx.now)
    })
}

/// [`plan_tick_edit`] on a path as the tool gave it (absolute or relative to `dir`).
fn plan_tick_in(dir: &Path, tool: &str, path: &str, input: &Value) -> bool {
    // Relative to the repo root (git top level), as `edit_reason` does, not the hook's cwd.
    let root = crate::ctx::git_out(dir, &["rev-parse", "--show-toplevel"])
        .map_or_else(|| dir.to_path_buf(), std::path::PathBuf::from);
    repo_relative(&root, dir, path).is_some_and(|rel| plan_tick_edit(tool, &rel, input))
}

/// An `Edit` that only ticks task checkboxes in a plan file: no lease is needed for it.
fn plan_tick_edit(tool: &str, path: &str, input: &Value) -> bool {
    let get = |k: &str| input.get(k).and_then(Value::as_str);
    tool == "Edit"
        && is_plan_file(path)
        && input.get("replace_all").and_then(Value::as_bool) != Some(true)
        && get("old_string")
            .zip(get("new_string"))
            .is_some_and(|(o, n)| is_plan_tick(o, n))
}

fn edit_reason(dir: &Path, path: &str) -> Result<Option<String>, String> {
    if super::session_start::unadopted(dir) {
        return Ok(None); // no leases here: no GitHub API call
    }
    let app = App::load(Some(dir)).map_err(|e| e.message)?;
    let Some(rel) = repo_relative(&app.ctx.dir, dir, path) else {
        return Ok(None); // outside the repo: not a claimable path
    };
    let file = cache::path(dir);
    let leases = cache::leases(&app, file.as_deref())?;
    // Assume the file exists first: git is asked only when that would deny.
    let me = &app.ctx.session;
    let Some(why) = edit_lock(&rel, true, &leases, me) else {
        return Ok(None);
    };
    if foreign_lock(&rel, &leases, me).is_some() {
        return Ok(Some(why));
    }
    let exists = app.ctx.dir.join(&rel).is_file() && on_base(&app.ctx.dir, &rel);
    if !exists {
        return Ok(None);
    }
    // The own-touch rule may rest on a stale cache (a claim that grew the touch): look again
    // before denying.
    match cache::refresh(&app, file.as_deref()) {
        Ok(fresh) => Ok(edit_lock(&rel, exists, &fresh, me)),
        Err(e) => {
            eprintln!(
                "bosun: could not re-check leases: {}; using the cached view",
                e.message
            );
            Ok(Some(why))
        }
    }
}

/// Whether `rel` is on the default branch (`origin/HEAD`, else `origin/main`, else
/// `origin/master`): a file added on this branch counts as new, so Write then Edit works. With
/// no such ref the file counts as new (fail open). Known limit: case-insensitive file systems.
fn on_base(root: &Path, rel: &str) -> bool {
    let known = |r: &str| git_out(root, &["rev-parse", "--verify", "--quiet", r]).is_some();
    let head = git_out(
        root,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    );
    let Some(base) = head
        .into_iter()
        .chain(["origin/main".to_string(), "origin/master".to_string()])
        .find(|r| known(r))
    else {
        return false;
    };
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", &format!("{base}:{rel}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `path` relative to the repo root, resolved with `canonicalize` when the file or its parent
/// exists, else `.` and `..` lexically; `None` when it lies outside the repo.
fn repo_relative(root: &Path, cwd: &Path, path: &str) -> Option<String> {
    use std::path::Component;
    let p = Path::new(path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    // The real path first, so symlinks and `link/../x` are judged by their target: the file
    // itself, else (a new file) its existing parent. Outside the repo: not a claimable path.
    let real = joined.canonicalize().ok().or_else(|| {
        let parent = joined.parent()?.canonicalize().ok()?;
        Some(parent.join(joined.file_name()?))
    });
    if let Some(real) = real {
        let r = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let rel = real.strip_prefix(&r).ok()?;
        let text = rel.to_str()?.replace('\\', "/");
        return (!rel.components().any(|c| c == Component::ParentDir)).then_some(text);
    }
    let mut abs = std::path::PathBuf::new();
    for c in joined.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !abs.pop() {
                    return None;
                }
            }
            other => abs.push(other),
        }
    }
    let strip = |a: &Path, r: &Path| a.strip_prefix(r).ok().map(Path::to_path_buf);
    let rel = strip(&abs, root).or_else(|| {
        let (a, r) = (abs.canonicalize().ok()?, root.canonicalize().ok()?);
        strip(&a, &r)
    })?;
    let text = rel.to_str()?.replace('\\', "/");
    (!rel.components().any(|c| c == Component::ParentDir)).then_some(text)
}
