//! LEGACY JUDGE: main's guard (d90f917), vendored verbatim except renames and the removed test
//! modules. The live guard denies whatever this denies, except the enumerated intended allows
//! (`super::intended`). Never edit the rules here.
//!
//! What the PreToolUse hook refuses (spec §4 edit hook, label-lock; D13 merge-lock). Pure: the
//! hook passes the tool call, the repo-relative path, the current branch and the live leases.

use std::cell::{Cell, RefCell};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::core::glob;
use crate::core::lease::{Conflict, Lease};
use crate::core::meta::OPEN;

mod interp;
mod literals;
mod prose;
mod settings;
pub use interp::{Script, script_rules};
pub use settings::settings_rule;
mod writes;
pub use writes::bash_writes;

const MERGE_FIX: &str = "agents never merge, tag or force-push (spec D13): open the PR with \
    `bosun pr <issue>` and let `bosun pr <issue> --auto-merge` hand it to GitHub after the gates \
    (a PR with no task: `bosun pr --pr <n> --auto-merge`)";

/// Whether a refusal reason is about a merge, tag or push, so that the "never merge, tag or
/// force-push" advice fits it. Reasons that carry their own fix, or are about something else,
/// do not get it.
fn tail_applies(why: &str) -> bool {
    const OTHER: [&str; 6] = [
        "xargs",
        "cannot see",
        "alias",
        "secrets and variables",
        "awk",
        "workflow run",
    ];
    why != PUSH_CONFIG && why != UNREAD_PUSH && !OTHER.iter().any(|o| why.contains(o))
}

/// The tools that write one file, and the input key that names it.
pub fn edited_path(tool: &str, input: &Value) -> Option<String> {
    let key = match tool {
        "Edit" | "Write" | "MultiEdit" => "file_path",
        "NotebookEdit" => "notebook_path",
        _ => return None,
    };
    input.get(key)?.as_str().map(str::to_string)
}

/// Edit-lock: a path inside a live lease of another session. The edited path is a literal
/// path (`app/[id]/page.tsx` has brackets), so each lease glob is matched against it and it is
/// never parsed as a glob itself. A lease with a bad glob covers everything. An agent may edit
/// the paths its own orchestrator leased (`S/task` runs inside `S`); see spec §4. A session that
/// holds a lease with a touch list must also stay inside it (`own_touch_lock`); `exists` says
/// whether the path is already a file (the hook checks, core stays pure).
pub fn edit_lock(path: &str, exists: bool, leases: &[Lease], me: &str) -> Option<String> {
    foreign_lock(path, leases, me).or_else(|| own_touch_lock(path, exists, leases, me))
}

/// The leases held by exactly `me` that list a touch. The hook sees only the plain session id,
/// so a per-task lease (`S/x`) never restricts `S`, and a wave orchestrator edits freely.
fn own_leases<'a>(leases: &'a [Lease], me: &str) -> Vec<&'a Lease> {
    leases
        .iter()
        .filter(|l| l.session == me && !l.touch.is_empty())
        .collect()
}

/// An existing file that no own glob (nor a lockfile companion) covers is refused. Fragments and
/// new files are free. A bad glob in an own lease covers nothing.
fn own_touch_lock(path: &str, exists: bool, leases: &[Lease], me: &str) -> Option<String> {
    let own = own_leases(leases, me);
    let first = own.first()?;
    if !exists || crate::core::touch::is_fragment(path) {
        return None;
    }
    let touch: Vec<String> = own.iter().flat_map(|l| l.touch.clone()).collect();
    let all = crate::core::touch::companions(&touch);
    let valid: Vec<glob::Glob> = all
        .iter()
        .filter_map(|g| glob::Glob::parse(g).ok())
        .collect();
    if valid.iter().any(|g| g.matches(path)) {
        return None;
    }
    let bad = if valid.len() < all.len() {
        " One of your globs is invalid: re-claim with valid globs."
    } else {
        ""
    };
    Some(format!(
        "{path} is outside your lease (touch: {}). Add it with `bosun claim {} --touch {path}` \
         (exit 3 if another session holds it), or leave the file alone.{bad}",
        touch.join(", "),
        first.issue
    ))
}

/// The first half of `edit_lock`: the path lies in a live lease of another session.
pub fn foreign_lock(path: &str, leases: &[Lease], me: &str) -> Option<String> {
    let hit = leases
        .iter()
        .filter(|l| !is_mine(&l.session, me))
        .find_map(|l| {
            let theirs = match glob::parse_all(&l.touch) {
                Ok(globs) => globs.iter().find(|g| g.matches(path))?.as_str().to_string(),
                Err(_) => "**".to_string(),
            };
            Some(Conflict {
                issue: l.issue,
                session: l.session.clone(),
                mine: path.to_string(),
                theirs,
                expires: l.expires,
            })
        })?;
    Some(format!(
        "{path} is claimed: {}. Claim your own task with `bosun claim`, or wait for that lease \
         (`bosun wait --issue {}`)",
        hit.describe(),
        hit.issue
    ))
}

/// A lease holder whose files `me` may edit: itself, its orchestrator (`S` for `S/task`), or a
/// per-task session of its own (`S/19` for the hook, which sees only the plain `S`).
fn is_mine(holder: &str, me: &str) -> bool {
    let orch = crate::core::lease::orchestrator_of;
    holder == me || holder == orch(me) || orch(holder) == me
}

/// The checked-out branch of the git target a command names, given as `git` global options
/// (`-C dir`, `--git-dir=..`, `--work-tree=..`) applied after the hook's own directory; an
/// empty list is the hook's directory. `None` when it cannot be read.
pub type BranchAt<'a> = &'a dyn Fn(&[String]) -> Option<String>;

/// The read-only tools of the GitHub MCP server (names without `mcp__github__`), which never set a
/// label. A new tool is not on the list until someone reads what it does.
const READ_ONLY_GITHUB: &[&str] = &[
    "get_me",
    "get_file_contents",
    "get_commit",
    "get_label",
    "get_tag",
    "get_latest_release",
    "get_release_by_tag",
    "get_check_run",
    "get_job_logs",
    "get_team_members",
    "get_teams",
    "get_copilot_job_status",
    "list_branches",
    "list_commits",
    "list_issues",
    "list_issue_types",
    "list_issue_fields",
    "list_pull_requests",
    "list_releases",
    "list_tags",
    "list_repository_collaborators",
    "actions_list",
    "actions_get",
    "search_code",
    "search_commits",
    "search_issues",
    "search_pull_requests",
    "search_repositories",
    "search_users",
    "issue_read",
    "pull_request_read",
];

/// Label-lock and merge-lock; `None` allows the call. `branch_at` reads the checked-out branch
/// of the directory a git command acts on (after `cd dir &&` and `git -C dir`), and
/// `default_branch` is the repo's default branch; `own` is the session id (identity allowance).
pub fn tool_rules(
    tool: &str,
    input: &Value,
    branch_at: BranchAt,
    default_branch: &str,
    own: &str,
) -> Option<String> {
    if tool == "Bash" {
        let command = input.get("command").and_then(Value::as_str).unwrap_or("");
        if let Some(why) = identity_tamper(command, own) {
            return Some(why);
        }
        return bash_rules(command, branch_at, default_branch);
    }
    if let Some(name) = tool.strip_prefix("mcp__github__") {
        if matches!(name, "merge_pull_request" | "enable_pr_auto_merge") {
            return Some(format!("`{tool}` is refused: {MERGE_FIX}"));
        }
        // Reading or listing sets no label, even when the filter names a state label. Only known
        // read-only tools are skipped; a tool not listed is judged below (fails closed).
        if READ_ONLY_GITHUB.contains(&name) {
            return None;
        }
        // A label tool is judged on its whole input; any other tool only on its label fields,
        // so a PR or issue body that names a state label in prose is not a label edit.
        let set = if name.contains("label") {
            input.to_string()
        } else {
            label_fields(input)
        };
        if set.contains(STATE) {
            return Some(format!(
                "`{tool}` sets a {STATE}* label and is refused: {LABEL_FIX}"
            ));
        }
    }
    None
}

/// The marker syntax bosun's parsers read (owner checklist `bosun:prereq|prereqs`, task and epic
/// metadata): an HTML comment opener, then the name, then a space, `{` or the comment end.
/// Prose that only mentions `bosun:task`, and `bosun:task-pr`, are not markers.
static MARKER: LazyLock<Regex> = LazyLock::new(|| marker_re(&format!("{NOT_TASK}|task")));

/// The marker names a registered child may not write; [`MARKER`] is these plus `task`, so the
/// two cannot drift.
const NOT_TASK: &str = "prereqs?|epic";

fn marker_re(names: &str) -> Regex {
    Regex::new(&format!(r"<!--\s*bosun:(?:{names})(?:[\s{{]|-->|$)")).expect("marker regex")
}

/// MCP tools that write an issue body or comment, and the input fields that hold the text.
const ISSUE_TEXT_TOOLS: [&str; 4] = [
    "issue_write",
    "update_issue",
    "add_issue_comment",
    "update_issue_comment",
];

/// The same without `task`: what a registered child may not write either.
static MARKER_NOT_TASK: LazyLock<Regex> = LazyLock::new(|| marker_re(NOT_TASK));

/// `task_ok`: the `task` marker does not count (a registered child may write it).
fn has_marker(text: &str, task_ok: bool) -> bool {
    if task_ok { &MARKER_NOT_TASK } else { &MARKER }.is_match(text)
}

/// What a registered child may write: one new issue carrying one `task` marker.
#[derive(Debug, PartialEq)]
pub struct ChildTask {
    /// The epic the marker names; the child must hold its spawn lease.
    pub epic: u64,
    /// The `(owner, repo)` an MCP call targets; `None` for `gh issue create` (this repo).
    pub repo: Option<(String, String)>,
}

/// Metadata keys a child may set. `allow` and `capabilities` widen what the gates let the task
/// do (test-integrity grants, the prod-write floor), so only a plan PR may set them.
const CHILD_KEYS: [&str; 7] = ["item", "task", "epic", "touch", "depends", "kind", "risk"];

/// Is `write` a registered child's creation of ONE new issue with ONE well-formed `task`
/// marker? Only `gh issue create` without `-R/--repo` (no nested shell, no other issue write in
/// the command) and the MCP `issue_write` with method `create` qualify; edits, comments, API
/// calls and other repos never do. The marker must be JSON with only [`CHILD_KEYS`] and an
/// `epic`; the caller checks that the child's spawn lease is on that epic.
pub fn child_task_create(tool: &str, input: &Value) -> Result<ChildTask, String> {
    let (text, repo) = match tool {
        "mcp__github__issue_write" => {
            let create = input.get("method").and_then(Value::as_str) == Some("create")
                && input.get("issue_number").is_none();
            if !create {
                return Err("a child may only create a new issue with the task marker".into());
            }
            let field = |k: &str| input.get(k).and_then(Value::as_str).map(String::from);
            let repo = field("owner").zip(field("repo"));
            (
                field("body").unwrap_or_default(),
                Some(repo.ok_or("the call names no owner/repo")?),
            )
        }
        "Bash" => {
            let command = input.get("command").and_then(Value::as_str).unwrap_or("");
            if !nested(command).is_empty() {
                return Err("a child may not write the marker through a nested shell".into());
            }
            let words = shell_words(command);
            // The whole command is the one `gh issue create`: no `cd dir &&` or other segment
            // that could change which repo `gh` acts on, and no `$` or backtick that expands to
            // text the checks below never see (`$'\x3c'`, `$VAR`).
            let segs: Vec<&[String]> = words.split(|w| w == SEP).collect();
            let [seg] = segs.as_slice() else {
                return Err("a child's command is the single `gh issue create`".into());
            };
            if !writes_issue_text(seg) {
                return Err("a child may only run `gh issue create` with the marker".into());
            }
            if seg.iter().any(|w| w.contains('$') || w.contains('`')) {
                return Err("a child's `gh issue create` may not use `$` or backticks".into());
            }
            // `gh` itself must open the command: not `./gh` or a path, and no variable prefix
            // (`GH_REPO=evil/x gh ...`, `GH_HOST=`) or wrapper that redirects it.
            let at = seg.first().filter(|w| w.as_str() == "gh").map(|_| 0);
            let ok = at.is_some_and(|at| {
                seg.get(at + 1).map(String::as_str) == Some("issue")
                    && seg.get(at + 2).map(String::as_str) == Some("create")
            });
            if !ok {
                return Err("a child may only run `gh issue create` with the marker".into());
            }
            if seg
                .iter()
                .any(|a| a == "-R" || a.starts_with("-R") || a.starts_with("--repo"))
            {
                return Err("a child may only create issues in this repo (no -R/--repo)".into());
            }
            (seg.join(" "), None)
        }
        _ => return Err("a child may only create an issue with the task marker".into()),
    };
    if MARKER_NOT_TASK.is_match(&text) || MARKER.find_iter(&text).count() != 1 {
        return Err("a child may write exactly one task marker and no other".into());
    }
    // The marker bosun itself reads (`core::meta::parse_task_meta`): the last `OPEN`, up to the
    // next `-->`. Prose that merely names `bosun:task` cannot stand in for it, and a second
    // `OPEN` is refused, so the JSON checked here is the JSON bosun parses.
    if text.matches(OPEN).count() != 1 {
        return Err("a child may write exactly one `<!-- bosun:task {...} -->` marker".into());
    }
    let after = text
        .rfind(OPEN)
        .map(|i| &text[i + OPEN.len()..])
        .and_then(|r| r.find("-->").map(|e| r[..e].trim()))
        .ok_or("the task marker has no closing -->")?;
    let meta: Value = serde_json::from_str(after).map_err(|e| format!("task marker JSON: {e}"))?;
    let obj = meta
        .as_object()
        .ok_or("the task marker is not a JSON object")?;
    if let Some(k) = obj.keys().find(|k| !CHILD_KEYS.contains(&k.as_str())) {
        return Err(format!(
            "a child may not set `{k}` in the task marker (allow and capabilities come from a plan PR)"
        ));
    }
    // Kinds with a lighter gate path (a main fix, a process fix) are for a plan PR to grant.
    if let Some(kind) = obj.get("kind").and_then(Value::as_str)
        && !["feature", "bookkeeping", "flake"].contains(&kind)
    {
        return Err(format!(
            "a child may not set kind `{kind}` (only feature, bookkeeping or flake)"
        ));
    }
    let epic = obj
        .get("epic")
        .and_then(Value::as_u64)
        .ok_or("the task marker needs the `epic` you were spawned for")?;
    Ok(ChildTask { epic, repo })
}

/// Does one simple command (words, quotes removed) write an issue body or comment through
/// `gh`, `hub` or `curl`?
fn writes_issue_text(words: &[String]) -> bool {
    let name = |w: &String| w.rsplit('/').next().unwrap_or(w).to_string();
    let Some(at) = words
        .iter()
        .position(|w| ["gh", "hub", "curl"].contains(&name(w).as_str()))
    else {
        return false;
    };
    let args: Vec<&str> = words[at + 1..].iter().map(String::as_str).collect();
    let has = |f: &[&str]| args.iter().any(|a| f.contains(a));
    let flag = |long: &str| args.iter().any(|a| a.starts_with(&format!("{long}=")));
    // `-b`, `-b TEXT`, `-bTEXT`, `--body`, `--body=TEXT` (not `--body-file`).
    let body = has(&["--body"])
        || flag("--body")
        || args
            .iter()
            .any(|a| a.starts_with("-b") && !a.starts_with("--"));
    // gh and curl use the last method flag; any write method among them counts as a write.
    let methods: Vec<&str> = args
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            if ["-X", "--method", "--request"].contains(a) {
                args.get(i + 1).copied()
            } else {
                a.strip_prefix("--method=")
                    .or_else(|| a.strip_prefix("--request="))
                    .or_else(|| a.strip_prefix("-X"))
            }
        })
        .collect();
    let write = |v: &str| ["POST", "PATCH", "PUT"].contains(&v.to_ascii_uppercase().as_str());
    let explicit = (!methods.is_empty()).then(|| methods.iter().any(|m| write(m)));
    if name(&words[at]) == "curl" {
        return explicit == Some(true)
            || args.iter().any(|a| {
                a.starts_with("-d") || a.starts_with("--data") || a.starts_with("--json")
            });
    }
    match args.first().copied() {
        Some("issue") => has(&["edit", "comment", "create", "close"]) && body,
        Some("pr") => has(&["comment"]) && body,
        Some("api") if args.get(1) == Some(&"graphql") => args
            .iter()
            .any(|a| a.to_ascii_lowercase().contains("mutation")),
        Some("api") => match explicit {
            Some(w) => w,
            None => {
                has(&["-f", "-F", "--field", "--raw-field"])
                    || flag("--field")
                    || flag("--raw-field")
            }
        },
        _ => false,
    }
}

/// Does `command`, or a command nested in it (`bash -c`, `eval`, `$( )`, groups), write issue
/// text that carries a marker?
fn writes_marker(command: &str, depth: usize, task_ok: bool) -> bool {
    depth <= MAX_DEPTH
        && (shell_words(command)
            .split(|w| w == SEP)
            .any(|seg| writes_issue_text(seg) && has_marker(&seg.join(" "), task_ok))
            || nested(command)
                .iter()
                .any(|n| writes_marker(n, depth + 1, task_ok)))
}

/// Markers-lock (design F4): refuses a write of an issue body or comment that carries a bosun
/// marker (`MARKER`: `<!-- bosun:prereq|prereqs|task|epic`; prose that names one is allowed).
/// Only writes are judged: `gh|hub issue edit|comment|create|close` and `gh pr comment` with
/// `--body`/`-b` (also `-bTEXT`), `gh|hub api` with a write method or field flag (never with an
/// explicit GET; with several method flags any write method counts; `gh api graphql` only with a
/// `mutation`), `curl` with a write method or a data
/// flag (`-d`, `--data*`, `--json`), the same inside `bash -c`, `eval` and `$( )`, and the MCP
/// tools in `ISSUE_TEXT_TOOLS` (their `body` and `comment` fields only). Best effort. Known and
/// accepted bypasses: `--body-file`, `--input`, `curl` or `gh api` with the text in a variable or
/// a file, a marker split by quotes (`'bosun:'"task"`) or built by `printf` in `$( )`, a
/// `gh api graphql -F query=@file` mutation (the text is in the file), wget,
/// httpie and python urllib. The real guarantees are elsewhere: bosun re-reads the checklist and
/// trusts only its own writes.
pub fn prereqs_lock(tool: &str, input: &Value) -> Option<String> {
    prereqs_lock_for(tool, input, false)
}

/// Answers-lock (spec 14): refuses an edit tool on `<git common dir>/bosun/answers.json`, and any
/// Bash command whose text names `answers.json` or the state dir `.git/bosun` (any case, path
/// normalised, also inside `bash -c`, `python -c`, a `cd` chain or a `$d` path), unless it is
/// purely read-only ([`answers_read_only`]). A hint, not a seal: it stops confused workers,
/// not liars (quoting can hide a name). The PostToolUse hook checks the transcript; the strong
/// path is a signed approval or the owner's own comment.
pub fn answers_lock(tool: &str, input: &Value) -> Option<String> {
    let hit = match tool {
        "Bash" => {
            let cmd = input.get("command").and_then(Value::as_str).unwrap_or("");
            names_answers(cmd) && !answers_read_only(cmd) && !names_only_in_text(cmd)
        }
        _ => edited_path(tool, input).is_some_and(|p| is_answers_path(&p)),
    };
    hit.then(|| {
        "the owner's answers file is written only by the PostToolUse hook for AskUserQuestion, \
         and ~/.claude/projects (session transcripts) only by Claude Code; do not write them (reading is fine). Ask the owner with `bosun approve --list` \
         and let the answer arrive through the tool"
            .to_string()
    })
}

/// Only read-only viewers (`cat`, `jq`, `less`, `head`, `tail`, `grep`, `rg`, `ls`, `wc`, `stat`,
/// `test`, `[`, `cd`, `python3 -m json.tool`), no write redirect (`2>/dev/null`, `>&2` are not
/// writes), no `$(...)` or backticks.
fn answers_read_only(command: &str) -> bool {
    let mut text = command.to_string();
    for ok in ["2>/dev/null", ">/dev/null", "2>&1", "1>&2", ">&2"] {
        text = text.replace(ok, " ");
    }
    if text.contains(['`', '>']) || text.contains("$(") {
        return false;
    }
    rough_segments(&text).iter().all(|seg| {
        let words = shell_words(seg);
        let mut it = words.iter().filter(|w| *w != SEP && !w.contains('='));
        let Some(first) = it.next() else { return true };
        match first.rsplit('/').next().unwrap_or(first) {
            "python3" | "python" => {
                it.next().is_some_and(|w| w == "-m")
                    && it.next().is_some_and(|w| w == "json.tool")
                    && it.filter(|w| !w.starts_with('-')).count() <= 1
            }
            c => matches!(
                c,
                "cat"
                    | "jq"
                    | "less"
                    | "head"
                    | "tail"
                    | "grep"
                    | "rg"
                    | "ls"
                    | "wc"
                    | "stat"
                    | "test"
                    | "["
                    | "cd"
                    | "echo"
                    | "printf"
            ),
        }
    })
}

/// `.../bosun/answers.json`, the adopted-check cache `.../bosun/adopted`, or anything under `.../.claude/projects/` once lexically normalised.
fn is_answers_path(path: &str) -> bool {
    let norm = format!("/{}", lexical(&path.replace('\\', "/")).to_lowercase());
    norm.ends_with("/bosun/answers.json")
        || norm.ends_with("/bosun/adopted")
        || norm.contains("/.claude/projects/")
}

/// Only a `git commit` or `gh pr|issue create|edit|comment` message, title or body names the file
/// (`-m`, `--message`, `-b`, `--body`, `--title`, `-t`); no `$(...)` or backticks, and the text
/// holds no `<` or `>` (`-m "x">answers.json` is one word, a redirect).
fn names_only_in_text(cmd: &str) -> bool {
    if cmd.contains(['`']) || cmd.contains("$(") {
        return false;
    }
    shell_words(cmd).split(|w| w == SEP).all(|g| {
        let msg = matches!(
            g.iter().map(String::as_str).collect::<Vec<_>>()[..],
            ["git", "commit", ..] | ["gh", "pr" | "issue", "create" | "edit" | "comment", ..]
        );
        let mut prev = "";
        let rest = g.iter().all(|w| {
            let text = ["-m", "--message", "-b", "--body", "--title", "-t"].contains(&prev);
            prev = w;
            (text && !w.contains(['<', '>'])) || !names_answers(w)
        });
        rest && (msg || !names_answers(&g.join(" ")))
    })
}

/// Whether the command text names `answers.json` or the state dir `.git/bosun`.
fn names_answers(command: &str) -> bool {
    let mut t = command.to_lowercase().replace('\\', "/");
    while t.contains("//") || t.contains("/./") {
        t = t.replace("//", "/").replace("/./", "/");
    }
    let state = |s: &str| s.contains(".git/bosun") || s.contains(".claude/projects");
    t.contains("answers.json")
        || state(&t)
        || shell_words(&t)
            .iter()
            .any(|w| state(&lexical(w.trim_start_matches(['<', '>', '&']))))
}

/// Bosun-dir lock: `rm`, `shred`, `mv` (source paths) and `git rm|mv` (also `--cached`) on `.bosun` or
/// under it are refused in a Bash command (also inside `bash -c` and `eval`): removing
/// `.bosun/version` would switch the guards off. Best effort: a path built in a variable or glob is missed.
pub fn bosun_dir_lock(tool: &str, input: &Value) -> Option<String> {
    let c = (tool == "Bash").then(|| input.get("command").and_then(Value::as_str))??;
    let why = "removing or moving `.bosun/` is refused: it holds the repo's bosun setup; change \
        it in a PR the owner approves";
    removes_bosun_dir(c, 0).then(|| why.to_string())
}

fn is_bosun_dir_path(w: &str) -> bool {
    // Case-insensitive: a case-insensitive filesystem treats `.BOSUN` as the same directory.
    let n = lexical(w.trim_end_matches('/')).to_lowercase();
    n == ".bosun" || n.starts_with(".bosun/") || n.ends_with("/.bosun") || n.contains("/.bosun/")
}

fn removes_bosun_dir(command: &str, depth: usize) -> bool {
    let words = shell_words(command);
    // Whether a `cd`/`pushd` put the shell in `.bosun` (relative paths then name its files).
    let mut in_bosun = false;
    for seg in words.split(|w| w == SEP) {
        let (name, args, runner_dirs) = writes::command_of(seg);
        // `env -C .bosun rm x`, `sudo -D .bosun rm x`: the runner moves the command into `.bosun`.
        let here = in_bosun || runner_dirs.iter().any(|d| is_bosun_dir_path(d));
        let (kind, args) = match name {
            "cd" | "pushd" => {
                let to = args.iter().find(|a| !a.starts_with('-')).copied();
                in_bosun = match to {
                    Some(t) if is_bosun_dir_path(t) => true,
                    Some(t) => in_bosun && !t.starts_with(['/', '~']),
                    None => false,
                };
                continue;
            }
            "find" => {
                let roots = args
                    .iter()
                    .skip_while(|a| matches!(**a, "-H" | "-L" | "-P"))
                    .take_while(|a| !a.starts_with('-') && !matches!(**a, "(" | "!"));
                let destructive = args.iter().any(|a| {
                    matches!(
                        *a,
                        "-delete"
                            | "-exec"
                            | "-execdir"
                            | "-ok"
                            | "-okdir"
                            | "rm"
                            | "unlink"
                            | "rmdir"
                            | "mv"
                            | "shred"
                    )
                });
                if destructive && (here || roots.into_iter().any(|r| is_bosun_dir_path(r))) {
                    return true;
                }
                continue;
            }
            "rm" | "unlink" | "rmdir" | "mv" | "shred" => (name, args),
            "git" => match args.iter().position(|w| matches!(*w, "rm" | "mv")) {
                Some(k) => {
                    // `git -C .bosun rm x`: the directory git moves to, before the subcommand.
                    let moved = args[..k]
                        .windows(2)
                        .any(|p| matches!(p[0], "-C" | "--work-tree") && is_bosun_dir_path(p[1]))
                        || args[..k].iter().any(|a| {
                            a.strip_prefix("--work-tree=")
                                .is_some_and(is_bosun_dir_path)
                        });
                    if moved {
                        return true;
                    }
                    (args[k], args[k + 1..].to_vec())
                }
                None => continue,
            },
            _ => continue,
        };
        // `mv a b`: the last path is the destination, the rest are sources.
        // `mv -t DIR a b` / `--target-directory DIR`: the destination is the option's value, every
        // other path is a source.
        let mut by_option = false;
        let mut paths: Vec<&&str> = Vec::new();
        let mut skip_value = false;
        for a in &args {
            if std::mem::take(&mut skip_value) {
                continue;
            }
            if kind == "mv" && a.starts_with("--") && a.len() > 2 {
                if let Some(value) = writes::target_directory_option(a) {
                    by_option = true;
                    skip_value = value.is_none();
                }
            } else if kind == "mv" && a.starts_with('-') && !a.starts_with("--") && a.len() > 1 {
                // Clustered short flags: `-ft DIR` takes the next word, `-tDIR` carries it.
                if let Some(k) = a.find('t') {
                    by_option = true;
                    skip_value = k == a.len() - 1;
                }
            } else if !a.starts_with('-') {
                paths.push(a);
            }
        }
        let n = if by_option {
            paths.len()
        } else {
            paths.len() - usize::from(kind == "mv" && paths.len() > 1)
        };
        let relative = |p: &str| here && !p.starts_with(['/', '~']);
        if paths[..n]
            .iter()
            .any(|p| is_bosun_dir_path(p) || relative(p))
        {
            return true;
        }
    }
    depth < 3
        && word_scripts(command)
            .iter()
            .any(|s| removes_bosun_dir(s, depth + 1))
}

/// [`prereqs_lock`] for a caller that may write the `task` marker (`task_ok`: a registered child
/// session, spec §4): the checklist and epic markers stay refused.
pub fn prereqs_lock_for(tool: &str, input: &Value, task_ok: bool) -> Option<String> {
    let hit = match tool.strip_prefix("mcp__github__") {
        Some(name) => {
            ISSUE_TEXT_TOOLS.contains(&name)
                && ["body", "comment"]
                    .iter()
                    .filter_map(|k| input.get(k).and_then(Value::as_str))
                    .any(|t| has_marker(t, task_ok))
        }
        None if tool == "Bash" => {
            let command = input.get("command").and_then(Value::as_str).unwrap_or("");
            has_marker(command, task_ok)
                && !read_only(command, 0)
                && writes_marker(command, 0, task_ok)
        }
        None => false,
    };
    hit.then(|| {
        "bosun's markers (owner checklist, task and epic metadata) are written by bosun only \
         (bosun prereqs --post writes the checklist); fix the plan instead"
            .to_string()
    })
}

/// Subagent-lock (spec §4): a Claude Code subagent (the hook input carries an `agent_id`) runs
/// inside its parent's session, with its session id and environment, so in the cloud (where the
/// child flag is no evidence) only this tells it from the orchestrator itself. It may not run
/// `bosun merge` or sign an approval (`bosun approve` with `--sign-key`, `--via-signer` or
/// `--as`). Lexical and best effort, like the other locks: any `bosun` word followed in its
/// segment by `merge`, or by `approve` and a signing flag, is refused.
pub fn subagent_lock(tool: &str, input: &Value) -> Option<String> {
    if tool != "Bash" {
        return None;
    }
    let command = input.get("command").and_then(Value::as_str)?;
    let refuse = || {
        Some(
            "a subagent never merges or signs an approval (D-022: child sessions never merge): \
             report the PR to the orchestrator, which runs `bosun merge` or `bosun approve` \
             itself"
                .to_string(),
        )
    };
    // `merge`, or `approve` with a signing flag, among the words after `bosun`.
    let refused = |rest: &[String]| {
        let has = |w: &str| rest.iter().any(|r| r == w);
        let signs = ["--sign-key", "--via-signer", "--as"].iter().any(|f| {
            rest.iter()
                .any(|r| r == f || r.starts_with(&format!("{f}=")))
        });
        has("merge") || (has("approve") && signs)
    };
    for seg in command.split(['\n', ';', '&', '|', '(', ')', '`']) {
        let words = shell_words(seg);
        // `cargo run -p keel-bosun -- merge`: the package or binary named, the words after `--`.
        if words.iter().any(|w| base(w) == "cargo") {
            let named = words.iter().enumerate().any(|(i, w)| {
                let next = words.get(i + 1).map(String::as_str);
                let bosun = |v: &str| matches!(v, "keel-bosun" | "bosun");
                match w.as_str() {
                    "-p" | "--package" | "--bin" => next.is_some_and(bosun),
                    w => w
                        .strip_prefix("--package=")
                        .or_else(|| w.strip_prefix("--bin="))
                        .or_else(|| w.strip_prefix("-p"))
                        .is_some_and(bosun),
                }
            });
            // Without `--` the words follow the package name directly (cargo rejects that, but
            // refuse the intent): the whole segment is searched.
            let from = words.iter().position(|w| w == "--").map_or(0, |d| d + 1);
            if named && refused(&words[from..]) {
                return refuse();
            }
        }
        let Some(i) = words.iter().position(|w| base(w) == "bosun") else {
            continue;
        };
        if refused(&words[i + 1..]) {
            return refuse();
        }
    }
    // Fail closed when the command name may be built at run time (`B=bosun; $B merge`,
    // `"$(echo bosun)" merge`): a `bosun` text, an expansion, and the words anywhere. A text
    // heuristic, not a parser: a name split at run time (`B=bo; ${B}sun merge`, `eval "bo""sun"`)
    // passes it, and an unrelated command can be refused (`cd $R/bosun && git merge main`). The
    // guard is a speed bump for subagents; the merge itself is judged by risk-check and the gates.
    if command.contains("bosun") && command.contains(['$', '`']) && refused(&shell_words(command)) {
        return refuse();
    }
    None
}

const IDENTITY_FIX: &str = "the identity environment must not be touched (D-019): no setting, unsetting, exporting or reading by name of BOSUN_SESSION, BOSUN_ORIGIN, CLAUDE_CODE_CHILD_SESSION, CLAUDE_CODE_REMOTE_SESSION_ID or CLAUDE_CODE_REMOTE, no `${!x}`, no `env -i`, `env -`, `exec -c`; only `BOSUN_SESSION=<your session>/<task>` and `BOSUN_ORIGIN=routine` on a command that is not bosun merge, approve, verdict or join --origin";
const IDENTITY: &str = "BOSUN_SESSION BOSUN_ORIGIN CLAUDE_CODE_CHILD_SESSION CLAUDE_CODE_REMOTE_SESSION_ID CLAUDE_CODE_REMOTE";

/// The `<task>` of a `BOSUN_SESSION` value `<own>/<task>` (`[A-Za-z0-9._-]+`).
pub fn task_of<'a>(value: &'a str, own: &str) -> Option<&'a str> {
    let task = value.strip_prefix(own)?.strip_prefix('/')?;
    let ok = |c: char| c.is_ascii_alphanumeric() || "._-".contains(c);
    (!own.is_empty() && !task.is_empty() && task.chars().all(ok)).then_some(task)
}

/// D-019, everywhere (stops confused workers, not liars; spec 04): refuses `NAME=v`, or a name
/// beside a verb (export, unset, env, read, ...), for the five identity names (`CLAUDE_CODE_REMOTE`
/// too: in the cloud it makes the child flag no evidence), any `${!`, and
/// `env -i`, `env -`, `exec -c`; git, gh, rg, grep, echo text passes. Allowed: `BOSUN_SESSION=
/// <own>/<task>` and `BOSUN_ORIGIN=routine` (prefix or one `export`) when `own` is set and the
/// text has no `$`, `--origin`, merge, approve or verdict.
fn identity_tamper(text: &str, own: &str) -> Option<String> {
    let plain: String = text.chars().filter(|c| !"'\"\\".contains(*c)).collect();
    let named = |w: &str| IDENTITY.split(' ').any(|n| w.contains(n));
    let setter = |w: &str| w.split_once('=').is_some_and(|(n, _)| named(n));
    let allowed = |w: &str| match w.split_once('=') {
        Some(("BOSUN_SESSION", v)) => task_of(v, own).is_some(),
        Some(("BOSUN_ORIGIN", v)) => v == "routine",
        _ => false,
    };
    let no = ["$", "--origin", "merge", "approve", "verdict"];
    let may = !own.is_empty() && !no.iter().any(|w| plain.contains(w));
    let among = |list: &str, w: &str| list.split(' ').any(|c| c == base(w));
    let verb = |w: &str| {
        among(
            "export unset declare typeset readonly local printf read env eval",
            w,
        )
    };
    let mut bad = plain.contains("${!");
    for seg in plain.split([';', '&', '|', '\n']) {
        let split = |c: char| c.is_whitespace() || "()`".contains(c);
        let t: Vec<&str> = seg.split(split).filter(|w| !w.is_empty()).collect();
        let first = t.iter().position(|w| !w.contains('='));
        let quiet = first == Some(0)
            && among("git gh rg grep echo", t[0])
            && !seg.contains("$(")
            && !seg.contains('`');
        let ids: Vec<_> = t.iter().filter(|w| named(w)).collect();
        let hit = !quiet
            && (t.iter().any(|w| setter(w)) || (!ids.is_empty() && t.iter().any(|w| verb(w))));
        let clean = ids.iter().all(|w| allowed(w))
            && t.iter().filter(|w| verb(w)).all(|w| base(w) == "export")
            && !t.contains(&"-n");
        bad |= hit && !(may && clean);
        bad |= t.iter().enumerate().any(|(i, w)| match base(w) {
            "env" => drops_env(&t[i + 1..], 'i'),
            "exec" => drops_env(&t[i + 1..], 'c'),
            _ => {
                first == Some(i)
                    && w.contains('$')
                    && ['i', 'c'].iter().any(|b| drops_env(&t[i + 1..], *b))
            }
        });
    }
    bad.then(|| IDENTITY_FIX.to_string())
}

/// Whether the options of `env` (`bad` = `i`) or `exec` (`c`) drop the environment.
fn drops_env(args: &[&str], bad: char) -> bool {
    let mut it = args.iter();
    while let Some(&a) = it.next() {
        if a == "-" || (bad == 'i' && long_prefix(a, "--ignore-environment", 3)) {
            return true;
        }
        if a.starts_with("--") {
            let takes = ["--unset", "--chdir", "--argv0"];
            if !a.contains('=') && takes.iter().any(|f| long_prefix(a, f, 3)) {
                it.next();
            }
        } else if let Some(c) = a.strip_prefix('-') {
            for (n, ch) in c.char_indices() {
                if ch == bad {
                    return true;
                }
                if "uCa".contains(ch) {
                    if n + 1 == c.len() {
                        it.next();
                    }
                    break;
                }
            }
        } else if !a.contains('=') {
            return false;
        }
    }
    false
}

/// The values under any key whose name contains `label`, at any depth, as JSON text.
fn label_fields(v: &Value) -> String {
    match v {
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| {
                if k.to_ascii_lowercase().contains("label") {
                    v.to_string()
                } else {
                    label_fields(v)
                }
            })
            .collect(),
        Value::Array(a) => a.iter().map(label_fields).collect(),
        _ => String::new(),
    }
}

const STATE: &str = "bosun:state/";
const LABEL_FIX: &str = "state labels change only through bosun commands (spec §4); use \
    `bosun claim`, `bosun release`, `bosun pr` or `bosun verdict post|comment`";

fn label_reason() -> String {
    format!(
        "a command that names a {STATE}* label may run only read-only commands (grep, cat, jq, \
         git show|log|diff, gh issue|pr list|view, gh search, bosun ...) and no `$` expansion: \
         {LABEL_FIX}"
    )
}

/// Nested commands are checked first (merge-lock; the label-lock judges them too), to this depth:
/// each `$( )`, backtick span, `( )` group (jq parens too: quotes are not parsed), `sh -c` and
/// `eval` is a level. Deeper commands are refused with `too_deep`.
const MAX_DEPTH: usize = 8;

fn too_deep() -> String {
    format!(
        "commands nested deeper than {MAX_DEPTH} levels (`$( )`, backticks, `( )` groups, \
         `sh -c`, `eval`) are refused, since the guard cannot judge them: run the inner \
         commands first and pass their results in variables"
    )
}

fn bash_rules(command: &str, branch_at: BranchAt, default_branch: &str) -> Option<String> {
    decrypt_rule(command, 0)
        .or_else(|| destructive_rule(command, 0))
        .or_else(|| check(command, branch_at, default_branch, 0, None))
        .or_else(|| {
            let prose = prose::strip_prose(command);
            (prose.contains(STATE) && !read_only(&prose, 0)).then(label_reason)
        })
}

/// A `git push` the merge-lock lets through (push-gates, spec §7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Push {
    /// `-C` options of the shell's `cd`s and git's own `-C`; `None`: the guard cannot tell.
    pub dir: Option<Vec<String>>,
    /// The local refs pushed (refspec sources); empty: the current branch.
    pub srcs: Vec<String>,
}

/// Where the walk reports each push it lets through.
pub(crate) type PushSink<'a> = &'a dyn Fn(Push);

/// The sinks of shell pushes (their `cd`s are tracked) and of pushes found in interpreter code
/// (whose directory the scan cannot tell).
#[derive(Clone, Copy)]
pub(crate) struct Sinks<'a> {
    pub shell: PushSink<'a>,
    pub code: PushSink<'a>,
    /// A push read in quotes or a `( )` group, which the nested walk may report with its `cd`.
    pub outer: PushSink<'a>,
}

/// Every `git push` of `command` that the merge-lock allows, in shell scripts it runs and code
/// it runs too (the same walk as the merge-lock). A refused command may list fewer. Interpreter
/// code is scanned as words, so a push found only there is one push whose directory is unknown;
/// when the shell walk found pushes, the code scan (which sees the same text) adds none.
pub fn pushes(
    command: &str,
    read: &dyn Fn(&str) -> Option<Script>,
    default_branch: &str,
) -> Vec<Push> {
    let (shell, code, outer) = (
        RefCell::new(Vec::new()),
        Cell::new(false),
        RefCell::new(vec![]),
    );
    let sinks = Sinks {
        shell: &|p: Push| shell.borrow_mut().push(p),
        code: &|_: Push| code.set(true),
        outer: &|p: Push| outer.borrow_mut().push(p),
    };
    // The branch only decides refusals, which `tool_rules` already made.
    let branch_at = |_: &[String]| Some(String::new());
    let _ = check(command, &branch_at, default_branch, 0, Some(&sinks));
    let _ = interp::script_walk(command, read, &branch_at, default_branch, Some(&sinks));
    let shell = shell.into_inner();
    // An outer push is dropped only when the nested walk reported the same refs.
    let outer = outer.into_inner().into_iter();
    let kept = outer.filter(|p| !shell.iter().any(|s| s.srcs == p.srcs));
    let mut out: Vec<Push> = Vec::new();
    for p in shell.iter().cloned().chain(kept.collect::<Vec<_>>()) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    if out.is_empty() && code.get() {
        out.push(Push {
            dir: None,
            srcs: Vec::new(),
        });
    }
    out
}

fn check(
    command: &str,
    branch_at: BranchAt,
    default_branch: &str,
    depth: usize,
    sinks: Option<&Sinks>,
) -> Option<String> {
    if depth > MAX_DEPTH {
        return Some(too_deep());
    }
    // A fetch or pull next to other shell commands reads an environment the guard cannot see.
    if let Some(why) = fetch_not_plain(command)
        .filter(|_| depth == 0)
        .or_else(|| glob_rule(command, depth == 0))
    {
        return Some(format!("`{}` is refused ({why})", command.trim()));
    }
    for inner in nested(command) {
        if let Some(why) = check(&inner, branch_at, default_branch, depth + 1, sinks) {
            return Some(why);
        }
    }
    let here = Target {
        dir: Some(&[]),
        alone: false,
        branch_at,
        default_branch,
        sink: sinks.map(|s| s.code),
    };
    if let Some(why) = interp::inline_rule(&split_heredocs(command).0, &here) {
        return Some(why);
    }
    // `cd` moves later git commands: `Some(args)` as `-C` options, `None` when the directory
    // cannot be told (git commands then count as on the default branch: fail closed). A `cd`
    // counts only when the shell surely runs it in this shell: after the start, `;` or a
    // newline, or inside an `&&` chain that the git command continues; never in a pipeline
    // or a `( .. )` group. `GIT_DIR`, `GIT_WORK_TREE` or `core.worktree` make it unknown.
    let mut dir: Option<Vec<String>> = Some(Vec::new());
    let (mut prev, mut chained) = ("", false);
    for (segment, sep) in segments_with_sep(command) {
        let words: Vec<&str> = segment.split_whitespace().collect();
        if ["GIT_DIR", "GIT_WORK_TREE", "core.worktree"]
            .iter()
            .any(|v| segment.contains(v))
        {
            dir = None;
        }
        let start = segment.as_ptr() as usize - command.as_ptr() as usize;
        let before = &command[..start];
        if words.first() == Some(&"popd") {
            dir = None;
        } else if matches!(words.first(), Some(&("cd" | "pushd"))) {
            let grouped = before.matches('(').count() > before.matches(')').count()
                || segment.contains(['(', ')']);
            let runs = matches!(prev, "" | ";" | "\n" | "&&");
            let keeps = matches!(sep, "&&" | ";" | "\n" | "");
            dir = match (dir, cd_target(&words[1..])) {
                (Some(mut d), Some(t)) if runs && keeps && !grouped => {
                    d.extend(["-C".to_string(), t]);
                    chained |= prev == "&&";
                    Some(d)
                }
                _ => None,
            };
        } else {
            let target = Target {
                dir: dir.as_deref(),
                alone: depth == 0 && alone(command),
                branch_at,
                default_branch,
                // Inside quotes (`bash -c '..'`) or a `( )` group, the nested walk may report
                // the push with its own `cd`s; `pushes` keeps this one only if it does not.
                sink: sinks.map(|s| {
                    let opens = before.matches('(').count() > before.matches(')').count();
                    if opens || in_quotes(before) {
                        s.outer
                    } else {
                        s.shell
                    }
                }),
            };
            // A command name the shell expands (`$g`, `${G:-gh}`, `$(echo gh)`, a backtick) cannot
            // be read. Text inside quotes or a heredoc is data, not a command.
            let named = words
                .iter()
                .find(|w| !(w.contains('=') && !w.starts_with('-')) && !WRAPPERS.contains(w));
            if depth == 0
                && !in_quotes(before)
                && !command.contains("<<")
                && let Some(w) =
                    named.filter(|w| w.trim_start_matches(['"', '\'']).starts_with(['$', '`']))
                && let Some(name) = built_name_rule(w, segment, &target)
            {
                return Some(format!(
                    "`{}` is refused (its command name is built by the shell and could be \
                     `{name}`, which agents may not run to merge, tag or push): write the \
                     command name out, not a variable or substitution",
                    segment.trim()
                ));
            }
            if matches!(prev, "|" | "|&") && reads_pipe(&words) {
                return Some(format!(
                    "`{}` is refused (a script piped into a shell cannot be read): download it \
                     to a file, read it, then run the file",
                    segment.trim()
                ));
            }
            if let Some(why) = merge_rule(&words, segment, &target) {
                if why == PUSH_ALONE {
                    let fix = alone_fix(&words);
                    return Some(format!("`{}` is refused ({fix})", segment.trim()));
                }
                return Some(format!(
                    "`{}` is refused ({why}){}",
                    segment.trim(),
                    if why.contains("awk") {
                        ": run the command itself, not through awk's system()".to_string()
                    } else if tail_applies(why) {
                        format!(": {MERGE_FIX}")
                    } else {
                        String::new()
                    }
                ));
            }
        }
        // A `cd` that ran only if an `&&` chain held may have been skipped once the chain
        // ends: later commands then run where the shell started.
        if chained && !matches!(sep, "&&" | "") {
            dir = None;
        }
        prev = sep;
    }
    None
}

/// Label-lock (spec §4): a guard against accidental or casual state-label edits, not a
/// sandbox. A command that names a state label must be read-only throughout, and the lock
/// fails closed: every command in it, at every nesting level, must be on the allowlist
/// (`segment_ro`), with no `$` expansion outside single quotes; `bosun` is on it, so
/// `bosun claim 3 # bosun:state/x` passes.
///
/// A plain command (no `$( )`, backticks, `${ }`, `$'..'`; see `plain_segments`) is split with
/// shell quoting, so a quoted `|` or `(` in a grep or jq argument is data, and the body of a
/// heredoc with a quoted delimiter is data. Anything else is split at every `; & |` and
/// newline, quoted or not, any segment with `$` or a backtick is refused, and every `( )` group
/// and backtick span is judged on its own too: stricter, never looser.
///
/// Known misses: a label split by quotes or escapes (`"bosun:"state/x`, `bosun:state\/x`), a
/// label built at run time (`printf`), a label passed through a file, and repo or user config,
/// or files, written by the same or an earlier command (a git pager, diff driver or
/// fsmonitor, a script on `PATH`); writes into `.git/` or a gitconfig are refused, other
/// paths are not.
fn read_only(text: &str, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    if let Some(segments) = plain_segments(text) {
        return segments.iter().all(|s| {
            !s.grouped
                && !s.dollar
                && segment_ro(&s.words, s.glob)
                && s.subs.iter().all(|t| read_only(t, depth + 1))
        });
    }
    // Backticks nest only when escaped; unescaped, every real span is a gap between two
    // consecutive backticks. An odd count is an unterminated span.
    let ticks: Vec<&str> = text.split('`').collect();
    if text.contains("\\`") || ticks.len().is_multiple_of(2) {
        return false;
    }
    let gaps = ticks.get(1..ticks.len() - 1).unwrap_or_default();
    gaps.iter().all(|g| read_only(g, depth + 1))
        && nested(text).iter().all(|n| read_only(n, depth + 1))
        && rough_segments(text).iter().all(|seg| {
            let words: Vec<String> = shell_words(seg).into_iter().filter(|w| w != SEP).collect();
            !seg.contains(['$', '`'])
                && !seg.contains("()")
                && segment_ro(&words, seg.contains(['*', '?', '[', '{']))
        })
}

/// Split at every `;`, `|`, `&` and newline, quoted or not; `>&`, `<&`, `&>` and `>|` are
/// redirections, not separators.
fn rough_segments(text: &str) -> Vec<&str> {
    let b = text.as_bytes();
    let (mut out, mut start) = (Vec::new(), 0);
    for i in 0..b.len() {
        let redirect = match b[i] {
            b'&' => (i > 0 && matches!(b[i - 1], b'<' | b'>')) || b.get(i + 1) == Some(&b'>'),
            b'|' => i > 0 && b[i - 1] == b'>',
            b';' | b'\n' => false,
            _ => continue,
        };
        if !redirect {
            out.push(&text[start..i]);
            start = i + 1;
        }
    }
    out.push(&text[start..]);
    out
}

/// One simple command of a plain command line: its words with quotes removed; whether an
/// unquoted `(` or `)` appeared (a subshell, a function, `case`: never read-only); whether a
/// `$` appeared outside single quotes (a variable, `$[..]`: never read-only); and whether an
/// argument has an unquoted glob or brace character (it may expand to an option).
#[derive(Default)]
struct PlainSegment {
    words: Vec<String>,
    grouped: bool,
    dollar: bool,
    glob: bool,
    /// Command substitutions in the unquoted heredoc bodies of this command: they run.
    subs: Vec<String>,
}

/// The simple commands of a plain command line, split the way bash splits it: `'..'`, `".."`
/// and `\` quoting, `#` comments, `\`-newline joins, redirections (`>&`, `&>`, `>|`) are not
/// separators, and heredoc bodies are skipped. `None` for anything this does not model
/// exactly (command or parameter substitution, `$'..'`, an unterminated quote or heredoc, an
/// unquoted-delimiter heredoc body with a `$name`, `${` or an unterminated substitution): the caller then
/// splits more roughly. The substitutions of an unquoted heredoc body run, so they are kept in
/// `PlainSegment::subs` to be judged; the rest of the body is data.
fn plain_segments(text: &str) -> Option<Vec<PlainSegment>> {
    let c: Vec<char> = text.chars().collect();
    let (mut out, mut seg) = (Vec::new(), PlainSegment::default());
    let (mut word, mut started) = (String::new(), false);
    // Pending heredocs on this line: delimiter, `<<-` (strip tabs), quoted delimiter.
    let mut heredocs: Vec<(String, bool, bool)> = Vec::new();
    let flush = |word: &mut String, started: &mut bool, seg: &mut PlainSegment| {
        if *started {
            seg.words.push(std::mem::take(word));
            *started = false;
        }
    };
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        match ch {
            '\'' => {
                let end = c[i + 1..].iter().position(|&x| x == '\'')? + i + 1;
                word.extend(&c[i + 1..end]);
                started = true;
                i = end + 1;
                continue;
            }
            '"' => {
                let mut j = i + 1;
                loop {
                    match *c.get(j)? {
                        '"' => break,
                        '`' => return None,
                        '$' if matches!(c.get(j + 1), Some('(' | '{')) => return None,
                        '$' => {
                            seg.dollar = true;
                            word.push('$');
                        }
                        '\\' => {
                            match *c.get(j + 1)? {
                                '\n' => {}
                                n @ ('$' | '`' | '"' | '\\') => word.push(n),
                                n => word.extend(['\\', n]),
                            }
                            j += 2;
                            continue;
                        }
                        x => word.push(x),
                    }
                    j += 1;
                }
                started = true;
                i = j + 1;
                continue;
            }
            '\\' => {
                let n = *c.get(i + 1)?;
                if n != '\n' {
                    word.push(n);
                    started = true;
                }
                i += 2;
                continue;
            }
            '`' => return None,
            '$' if matches!(c.get(i + 1), Some('(' | '{' | '\'' | '"')) => return None,
            '$' => {
                seg.dollar = true;
                word.push('$');
                started = true;
            }
            '#' if !started => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '\n' => {
                flush(&mut word, &mut started, &mut seg);
                out.push(std::mem::take(&mut seg));
                i += 1;
                for (delim, tabs, quoted) in std::mem::take(&mut heredocs) {
                    let mut body = String::new();
                    loop {
                        if i >= c.len() {
                            return None;
                        }
                        let end = c[i..]
                            .iter()
                            .position(|&x| x == '\n')
                            .map_or(c.len(), |p| p + i);
                        let line: String = c[i..end].iter().collect();
                        i = (end + 1).min(c.len());
                        let bare = if tabs {
                            line.trim_start_matches('\t')
                        } else {
                            &line
                        };
                        // Bash joins a backslash-newline before it matches the delimiter.
                        if !quoted && (line.len() - line.trim_end_matches('\\').len()) % 2 == 1 {
                            return None;
                        }
                        if bare == delim {
                            // The body is data, but its substitutions run: judge them.
                            let subs = heredoc_subs(&body)?;
                            out.last_mut()?.subs.extend(subs);
                            break;
                        }
                        if !quoted {
                            body.push_str(&line);
                            body.push('\n');
                        }
                    }
                }
                continue;
            }
            ';' | '|' => {
                flush(&mut word, &mut started, &mut seg);
                out.push(std::mem::take(&mut seg));
            }
            '&' if c.get(i + 1) == Some(&'>') => flush(&mut word, &mut started, &mut seg),
            '&' => {
                flush(&mut word, &mut started, &mut seg);
                out.push(std::mem::take(&mut seg));
            }
            '(' | ')' => {
                flush(&mut word, &mut started, &mut seg);
                seg.grouped = true;
            }
            '<' | '>' => {
                flush(&mut word, &mut started, &mut seg);
                let rest: String = c[i..c.len().min(i + 3)].iter().collect();
                if rest == "<<<" {
                    seg.words.push(rest);
                    i += 3;
                    continue;
                }
                if rest.starts_with("<<") {
                    i += 2;
                    let tabs = c.get(i) == Some(&'-');
                    i += usize::from(tabs);
                    while matches!(c.get(i), Some(' ' | '\t')) {
                        i += 1;
                    }
                    let (mut delim, mut quoted) = (String::new(), false);
                    while let Some(&x) = c.get(i) {
                        match x {
                            '\'' | '"' => {
                                let end = c[i + 1..].iter().position(|&y| y == x)? + i + 1;
                                delim.extend(&c[i + 1..end]);
                                quoted = true;
                                i = end + 1;
                                continue;
                            }
                            '\\' => {
                                delim.push(*c.get(i + 1)?);
                                quoted = true;
                                i += 2;
                                continue;
                            }
                            x if x.is_whitespace() || ";&|<>()`$".contains(x) => break,
                            x => delim.push(x),
                        }
                        i += 1;
                    }
                    if delim.is_empty() {
                        return None;
                    }
                    seg.words.extend(["<<".to_string(), delim.clone()]);
                    heredocs.push((delim, tabs, quoted));
                    continue;
                }
                // `>`, `>>`, `>&`, `>|`, `<`, `<&`, `<>`: one operator word.
                let mut op = ch.to_string();
                if let Some(&n) = c.get(i + 1)
                    && (n == ch || matches!(n, '&' | '|') || (ch == '<' && n == '>'))
                {
                    op.push(n);
                }
                i += op.len();
                seg.words.push(op);
                continue;
            }
            x if x.is_whitespace() => flush(&mut word, &mut started, &mut seg),
            x => {
                seg.glob |= !seg.words.is_empty() && matches!(x, '*' | '?' | '[' | '{');
                word.push(x);
                started = true;
            }
        }
        i += 1;
    }
    if !heredocs.is_empty() {
        return None;
    }
    flush(&mut word, &mut started, &mut seg);
    out.push(seg);
    Some(out)
}

/// The command substitutions (`$( )` and backtick spans) in an unquoted heredoc body; the rest
/// of the body is text. A backslash escapes `$` and a backtick. `None` for anything not modelled
/// exactly (`$name`, `${ }`, an unterminated span, a span with a quote, `#`, `<<`, newline,
/// backslash or `case`): the caller then judges the whole command roughly.
fn heredoc_subs(body: &str) -> Option<Vec<String>> {
    let c: Vec<char> = body.chars().collect();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < c.len() {
        match c[i] {
            '\\' => i += 1,
            '`' => {
                let end = c[i + 1..].iter().position(|&x| x == '`')? + i + 1;
                out.push(c[i + 1..end].iter().collect::<String>());
                i = end;
            }
            // A variable may hold text that a later expansion runs (`${x@P}`, the F13 probes):
            // only a bare `$` (before a space or the end) is plain text.
            '$' if c
                .get(i + 1)
                .is_some_and(|n| *n != '(' && !n.is_whitespace()) =>
            {
                return None;
            }
            '$' if c.get(i + 1) == Some(&'(') => {
                let (mut level, mut j) = (0, i + 1);
                loop {
                    match *c.get(j)? {
                        '(' => level += 1,
                        ')' => level -= 1,
                        _ => {}
                    }
                    j += 1;
                    if level == 0 {
                        break;
                    }
                }
                out.push(c[i + 2..j - 1].iter().collect::<String>());
                i = j - 1;
            }
            _ => {}
        }
        i += 1;
    }
    // Fail closed: the span end above counts raw parens, so a span with anything that can hide
    // or fake a `)` (a quote, a comment, a nested heredoc, a newline, an escape, a `case`
    // pattern) is not modelled.
    let odd = |t: &String| {
        t.contains(['\'', '"', '#', '\\', '\n']) || t.contains("<<") || t.contains("case")
    };
    (!out.iter().any(odd)).then_some(out)
}

/// Variables an assignment-only segment may not set: they change which program runs, or make
/// an allowlisted command run one.
fn risky_var(name: &str) -> bool {
    matches!(
        name,
        "PATH"
            | "IFS"
            | "ENV"
            | "CDPATH"
            | "PAGER"
            | "MANPAGER"
            | "PROMPT_COMMAND"
            | "PS4"
            | "HOME"
    ) || ["LD_", "GIT_", "GH_", "LESS", "BASH", "XDG_"]
        .iter()
        .any(|p| name.starts_with(p))
}

/// The variable a `NAME=value` (or `NAME+=value`) word assigns.
fn assigned(word: &str) -> Option<&str> {
    let (name, _) = word.split_once('=')?;
    let name = name.strip_suffix('+').unwrap_or(name);
    let mut cs = name.chars();
    let first = cs.next()?;
    ((first.is_ascii_alphabetic() || first == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_'))
    .then_some(name)
}

/// The `git show|log|diff|blame|status` options the label-lock vouches for: none runs a
/// program or a pager. Matched exactly (git takes abbreviations, which are refused), the name
/// before any `=`.
const GIT_OPTIONS: &[&str] = &[
    "--",
    "--abbrev-commit",
    "--after",
    "--all",
    "--author",
    "--before",
    "--branch",
    "--cached",
    "--check",
    "--date",
    "--decorate",
    "--exit-code",
    "--first-parent",
    "--follow",
    "--format",
    "--graph",
    "--grep",
    "--max-count",
    "--merges",
    "--name-only",
    "--name-status",
    "--no-color",
    "--no-ext-diff",
    "--no-merges",
    "--no-patch",
    "--no-textconv",
    "--numstat",
    "--oneline",
    "--patch",
    "--porcelain",
    "--pretty",
    "--quiet",
    "--reverse",
    "--short",
    "--shortstat",
    "--since",
    "--staged",
    "--stat",
    "--until",
    "-b",
    "-p",
    "-q",
    "-s",
    "-w",
];

/// A vouched git option: one of `GIT_OPTIONS`, `-<n>` or `-n<n>`.
fn git_option_ok(arg: &str) -> bool {
    let name = arg.split_once('=').map_or(arg, |(k, _)| k);
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    GIT_OPTIONS.contains(&name)
        || arg.strip_prefix('-').is_some_and(digits)
        || arg.strip_prefix("-n").is_some_and(digits)
}

/// The label-lock allowlist. A segment is read-only when its command (quotes removed, no
/// path) is a text tool (grep, cat, jq, ...: no option of theirs runs a program), or, with no
/// glob or brace in its arguments (`glob`, which could expand to an option), `bosun`,
/// `git show|log|diff|blame|status` (only `-C dir` and `--no-pager` before it, only
/// `GIT_OPTIONS` after it), or read-only `gh` (`issue|pr list|view|status`, `pr diff|checks`,
/// `search`, `label list`). Assignments before a command are allowed only for `bosun`; a
/// segment of assignments alone is read-only unless it sets a `risky_var`. Keywords, groups,
/// functions, `xargs`, `less` (its `!`), `sort` and `rg` (options that run programs),
/// `git grep` (`-O`) and every other command are not.
fn segment_ro(words: &[String], glob: bool) -> bool {
    if words
        .iter()
        .any(|w| w.contains("/dev/tcp/") || w.contains("/dev/udp/"))
    {
        return false;
    }
    // Output redirected into git config (`.git/`, `gitconfig`, `git/config`) can make a later
    // allowlisted git command run a program.
    let into_git_config = words.iter().enumerate().any(|(i, w)| {
        let target = w.trim_start_matches(['>', '|', '&']);
        let target = match (w.starts_with('>'), target.is_empty()) {
            (false, _) => return false,
            (true, true) => words.get(i + 1).map_or("", String::as_str),
            (true, false) => target,
        };
        target.contains(".git/") || target.ends_with("gitconfig") || target.ends_with("git/config")
    });
    if into_git_config {
        return false;
    }
    let n = words.iter().take_while(|w| assigned(w).is_some()).count();
    let Some(cmd) = words.get(n) else {
        return !words.iter().filter_map(|w| assigned(w)).any(risky_var);
    };
    if n > 0 && cmd != "bosun" {
        return false;
    }
    let args: Vec<&str> = words[n + 1..].iter().map(String::as_str).collect();
    match cmd.as_str() {
        // `printf -v NAME` assigns a variable, as `NAME=` would, unchecked.
        "printf" => !args.iter().any(|a| a.starts_with("-v")),
        "grep" | "egrep" | "fgrep" | "cat" | "head" | "tail" | "wc" | "uniq" | "cut" | "tr"
        | "jq" | "diff" | "ls" | "test" | "[" | "true" | "false" | "echo" => true,
        _ if glob => false,
        "bosun" => true,
        "git" => {
            let mut i = 0;
            loop {
                match args.get(i) {
                    Some(&"-C") => i += 2,
                    Some(&"--no-pager") => i += 1,
                    _ => break,
                }
            }
            let rest = args.get(i + 1..).unwrap_or_default();
            let mut options = rest.iter().take_while(|a| **a != "--");
            matches!(
                args.get(i),
                Some(&("show" | "log" | "diff" | "blame" | "status"))
            ) && options.all(|a| !a.starts_with('-') || git_option_ok(a))
        }
        "gh" => matches!(
            args.as_slice(),
            ["issue", "list" | "view" | "status", ..]
                | ["pr", "list" | "view" | "status" | "diff" | "checks", ..]
                | ["search", ..]
                | ["label", "list", ..]
        ),
        _ => false,
    }
}

/// The commands inside `$(...)`, backticks, `( ... )` groups, `sh|bash|zsh -c STR` and
/// `eval STR`. Quotes are not honoured for groups, which only makes the check stricter.
fn nested(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = command.chars().collect();
    let (mut i, mut tick) = (0, None);
    while i < chars.len() {
        match chars[i] {
            '(' => {
                let (mut level, mut j) = (1, i + 1);
                while j < chars.len() && level > 0 {
                    match chars[j] {
                        '(' => level += 1,
                        ')' => level -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                let end = if level == 0 { j - 1 } else { j };
                out.push(chars[i + 1..end].iter().collect());
                i = j;
                continue;
            }
            '`' => match tick.take() {
                Some(start) => out.push(chars[start + 1..i].iter().collect()),
                None => tick = Some(i),
            },
            _ => {}
        }
        i += 1;
    }
    if let Some(start) = tick {
        out.push(chars[start + 1..].iter().collect());
    }
    out.extend(word_scripts(command));
    out
}

/// The scripts of `env -S`, `eval` and `sh|bash|zsh|dash|ksh -c` in a command.
fn word_scripts(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let words = shell_words(command);
    for (i, w) in words.iter().enumerate() {
        let name = w.rsplit('/').next().unwrap_or(w);
        if name == "env"
            && let Some(inner) = env_split_string(&words[i + 1..])
        {
            out.push(inner);
        } else if name == "eval" {
            let rest: Vec<&str> = words[i + 1..]
                .iter()
                .map(String::as_str)
                .take_while(|w| *w != SEP)
                .collect();
            out.push(rest.join(" "));
        } else if matches!(name, "sh" | "bash" | "zsh" | "dash" | "ksh") {
            let flag = words[i + 1..]
                .iter()
                .take_while(|w| **w != SEP)
                .position(|w| w.starts_with('-') && !w.starts_with("--") && w.contains('c'));
            if let Some(f) = flag
                && let Some(script) = words.get(i + 2 + f)
            {
                out.push(script.clone());
            }
        }
    }
    out
}

/// The command `env -S STR [ARGS]` runs (`-S` splits STR into words): STR and the words after
/// it, read through env's options (`-u NAME`, `-C DIR`, `-i`, assignments).
fn env_split_string(after: &[String]) -> Option<String> {
    let after: Vec<&str> = after
        .iter()
        .map(String::as_str)
        .take_while(|w| *w != SEP)
        .collect();
    let mut k = 0;
    while let Some(&w) = after.get(k) {
        let short = w.starts_with('-') && !w.starts_with("--");
        let value = if w == "--split-string" {
            Some("")
        } else if let Some(v) = w.strip_prefix("--split-string=") {
            Some(v)
        } else if short && !w.starts_with("-u") && !w.starts_with("-C") && w.contains('S') {
            w.split_once('S').map(|(_, v)| v)
        } else {
            None
        };
        if let Some(v) = value {
            let rest = after[k + 1..].join(" ");
            return Some(if v.is_empty() {
                rest
            } else {
                format!("{v} {rest}")
            });
        }
        k += match w {
            "-u" | "--unset" | "-C" | "--chdir" => 2,
            _ if w.starts_with('-') || w.contains('=') => 1,
            _ => return None,
        };
    }
    None
}

const SEP: &str = "\u{0}";

/// Split into words with shell quoting (`'..'`, `".."`, backslash); `;`, `&`, `|` and
/// newlines become `SEP` words.
fn shell_words(s: &str) -> Vec<String> {
    let (mut words, mut cur, mut quote, mut started) = (Vec::new(), String::new(), None, false);
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() || matches!(c, ';' | '&' | '|') => {
                if started {
                    words.push(std::mem::take(&mut cur));
                    started = false;
                }
                if !c.is_whitespace() || c == '\n' {
                    words.push(SEP.to_string());
                }
            }
            (None, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(cur);
    }
    words
}

/// Words that run the next word as a command (`sudo gh ...`) or open a shell group.
const WRAPPERS: &[&str] = &[
    "env", "command", "sudo", "nohup", "time", "exec", "nice", "{", "!", "then", "do", "else",
    "if", "while", "until", "builtin",
];

/// Commands that run the command after their options: name, options with a separate value,
/// positionals before the command (`timeout 5`), options that change directory.
#[rustfmt::skip]
const RUNNERS: &[(&str, &[&str], usize, &[&str])] = &[
    ("env", &["-u", "--unset", "-C", "--chdir", "-S", "--split-string"], 0, &["-C", "--chdir"]),
    ("sudo", &["-u", "--user", "-g", "--group", "-C", "--close-from", "-D", "--chdir", "-p", "--prompt", "-r", "--role", "-t", "--type", "-T", "--command-timeout", "-U", "--other-user", "-R", "--chroot", "-h", "--host", "-a", "--auth-type", "-c", "--login-class"], 0, &["-D", "--chdir"]),
    ("nice", &["-n", "--adjustment"], 0, &[]), ("command", &[], 0, &[]), ("exec", &["-a"], 0, &[]),
    ("nohup", &[], 0, &[]), ("setsid", &[], 0, &[]), ("time", &["-f", "--format", "-o", "--output"], 0, &[]),
    ("timeout", &["-k", "--kill-after", "-s", "--signal"], 1, &[]),
    ("stdbuf", &["-i", "--input", "-o", "--output", "-e", "--error"], 0, &[]),
    ("ionice", &["-c", "--class", "-n", "--classdata", "-p", "--pid", "-P", "--pgid", "-u", "--uid"], 0, &[]),
    ("xargs", &["-I", "-n", "-L", "-P", "-s", "-d", "-E", "-a", "--max-args", "--max-lines", "--max-procs", "--max-chars", "--delimiter", "--arg-file", "--eof", "--process-slot-var"], 0, &[]),
    ("doas", &["-u", "-C", "-a"], 0, &[]), ("unbuffer", &[], 0, &[]), ("busybox", &[], 0, &[]),
    ("chrt", &["-T", "--sched-runtime", "-P", "--sched-period", "-D", "--sched-deadline"], 1, &[]),
    // `-c` takes no value: the mask positional is the cpu list then.
    ("taskset", &[], 1, &[]),
    ("watch", &["-n", "--interval", "-q", "--equexit"], 0, &[]),
    // `FILE -c CMD`: `-c` is a flag here, so its value is the command.
    ("flock", &["-w", "--timeout", "--wait", "-E", "--conflict-exit-code"], 1, &[]),
    ("strace", &["-a", "-b", "-e", "-E", "-I", "-o", "-O", "-p", "-P", "-s", "-S", "-u", "-U", "-X", "--output", "--attach", "--string-limit", "--user", "--env", "--detach-on", "--columns", "--overhead", "--summary-sort-by", "--trace-path", "--summary-columns", "--const-print-style", "--trace", "--trace-fds", "--signal", "--signals", "--status", "--abbrev", "--verbose", "--raw", "--read", "--write", "--fault", "--inject", "--kvm", "--decode-pids"], 0, &[]),
    ("ltrace", &["-a", "-A", "-D", "-e", "-F", "-l", "-n", "-o", "-p", "-s", "-u", "-w", "-x", "--align", "--indent", "--library", "--output", "--config", "--where"], 0, &[]),
    ("unshare", &["-S", "--setuid", "-G", "--setgid", "-R", "--root", "-w", "--wd", "--propagation", "--setgroups", "--map-user", "--map-group", "--map-users", "--map-groups", "--monotonic", "--boottime"], 0, &["-w", "--wd"]),
    ("chroot", &["--userspec", "--groups"], 1, &[]),
    ("caffeinate", &["-t", "-w"], 0, &[]),
];

/// Runner options that change the root (`unshare -R dir`, `sudo --chroot dir`): the command
/// starts in the new `/`, read as `/` of this filesystem (`chroot` does the same).
const NEW_ROOT: &[&str] = &["-R", "--root", "--chroot"];

/// strace options with an optional value, which getopt takes only after `=`: a flag in
/// `RUNNERS`, and judged again as taking the next word (`--decode-fds all cmd`).
#[rustfmt::skip]
const STRACE_OPTIONAL: &[&str] = &["--quiet", "--silent", "--silence", "--decode-fds", "--timestamps", "--absolute-timestamps", "--syscall-times"];

/// The command after leading runners (`nice -n 5 sudo -u x grep ..`), the directories
/// their options move to (`env -C dir`, `--chdir=dir`, `sudo -D dir`, `unshare -w dir`, `/`
/// after a new root), and whether a runner joins the command into a shell string (`watch`).
fn unwrap_runners<'a>(mut cmd: &'a [&'a str]) -> (&'a [&'a str], Vec<&'a str>, bool) {
    let (mut dirs, mut shell) = (Vec::new(), false);
    while let Some(&(runner, vals, mut pos, cd)) = cmd.first().and_then(|w| {
        let w = w.rsplit('/').next().unwrap_or(w);
        RUNNERS.iter().find(|r| r.0 == w)
    }) {
        let mut i = 1;
        while let Some(&a) = cmd.get(i) {
            i += 1;
            if !a.starts_with('-') && assigned(a).is_none() {
                match pos {
                    0 => {
                        i -= 1;
                        break;
                    }
                    _ => pos -= 1,
                }
                continue;
            }
            let (opt, val) = match a.split_once('=') {
                Some((o, v)) if a.starts_with("--") => (o, Some(v)),
                _ if a.len() > 2 && a.get(..2).is_some_and(|o| vals.contains(&o)) => {
                    (&a[..2], Some(&a[2..]))
                }
                _ => (a, None),
            };
            let val = match val {
                None if vals.contains(&opt) => cmd.get(i).inspect(|_| i += 1).copied(),
                v => v,
            };
            if let Some(v) = val.filter(|_| cd.contains(&opt)) {
                dirs.push(v);
            }
            if val.is_some() && NEW_ROOT.contains(&opt) {
                dirs.push("/");
            }
        }
        // A runner with no command after it is the command (`command env`, `timeout 5 env`).
        if cmd.get(i).is_none_or(|w| redirect(w)) {
            break;
        }
        if runner == "chroot" {
            dirs.push("/");
        }
        // `watch` runs its words through `sh -c` (also judged with `-x`: fail closed).
        shell |= runner == "watch";
        cmd = &cmd[i..];
    }
    (cmd, dirs, shell)
}

/// Mutations and endpoints that merge, tag or move refs through the GitHub API.
const GRAPHQL_WRITES: &[&str] = &[
    "mergePullRequest",
    "enablePullRequestAutoMerge",
    "disablePullRequestAutoMerge",
    "enqueuePullRequest",
    "mergeBranch",
    "createRef",
    "updateRef",
    "deleteRef",
];
const HTTP_WRITES: &[&str] = &[
    "/merge",
    "/git/refs",
    "/git/tags",
    "/releases",
    "auto_merge",
    "automerge",
];

/// Where a git command acts: the `-C` options of earlier `cd`s (`None`: unknown), and how to
/// read the branch there.
#[derive(Clone, Copy)]
struct Target<'a> {
    dir: Option<&'a [String]>,
    /// The git command is the whole input (no other command, nesting, expansion or
    /// assignment), so the config the hook reads is the config git runs with.
    alone: bool,
    branch_at: BranchAt<'a>,
    default_branch: &'a str,
    sink: Option<PushSink<'a>>,
}

/// The segments of a command with the separator that ends each one (`&&`, `||`, `;`, `|`,
/// `&`, a newline, or `""` at the end). Quotes are not parsed; a quoted separator only makes
/// the check stricter.
fn segments_with_sep(command: &str) -> Vec<(&str, &'static str)> {
    let (b, mut out, mut start, mut i) = (command.as_bytes(), Vec::new(), 0, 0);
    while i < b.len() {
        let (sep, len): (&'static str, usize) = match (b[i], b.get(i + 1)) {
            (b'&', Some(b'&')) => ("&&", 2),
            (b'|', Some(b'|')) => ("||", 2),
            (b'&', _) => ("&", 1),
            (b'|', _) => ("|", 1),
            (b';', _) => (";", 1),
            (b'\n', _) => ("\n", 1),
            _ => {
                i += 1;
                continue;
            }
        };
        out.push((command[start..i].trim(), sep));
        i += len;
        start = i;
    }
    out.push((command[start..].trim(), ""));
    out.into_iter().filter(|(s, _)| !s.is_empty()).collect()
}

/// A literal path without its quotes; `None` for anything the shell would expand.
fn literal_path(word: &str) -> Option<String> {
    let w = word.trim_matches(|c| c == '"' || c == '\'');
    let expands = w.is_empty()
        || w.starts_with('~')
        || w == "-"
        || w.contains(['$', '`', '*', '?', '[', '{']);
    (!expands).then(|| w.to_string())
}

/// The directory of `cd ARGS`; `None` for `cd` alone, `cd -`, options or expansions.
fn cd_target(args: &[&str]) -> Option<String> {
    match args {
        ["--", one] => literal_path(one),
        [one] if !one.starts_with('-') => literal_path(one),
        _ => None,
    }
}

/// git subcommands that publish, tag or change a branch: with `xargs` their arguments come
/// from stdin, so the guard cannot judge them.
const XARGS_GIT: &[&str] = &[
    "push",
    "tag",
    "merge",
    "rebase",
    "reset",
    "cherry-pick",
    "commit",
];

fn merge_rule(words: &[&str], segment: &str, target: &Target) -> Option<&'static str> {
    // Leading assignments and wrappers, then runners (`timeout 5`, `xargs -n1`, `env -C dir`),
    // until the command itself.
    let runner = |w: &str| {
        RUNNERS
            .iter()
            .any(|r| r.0 == w.rsplit('/').next().unwrap_or(w))
    };
    // The command name a word runs: `\git`, `"git"`, `g''it` and `/usr/bin/git` run git.
    let name = |w: &str| {
        let even = |q: char| w.matches(q).count().is_multiple_of(2);
        let w = if even('\'') && even('"') {
            w.replace(['\'', '"', '\\'], "")
        } else {
            w.to_string()
        };
        w.rsplit('/').next().unwrap_or(&w).to_string()
    };
    // Bash expands `{a,b}` before the command runs: judge the words it makes (an unquoted empty
    // one is dropped). A word the guard cannot expand is refused where it can steer the command:
    // after a wrapper or runner (whose option values the guard may misread), as the command word
    // when its first word is git, gh, a shell, a wrapper or a runner, or in a git or gh command.
    let (mut grown, mut ex, mut unread) = (false, Vec::new(), Vec::new());
    for w in words {
        match braces(w, 0, 0) {
            Some(Ok(more)) => {
                ex.extend(more.into_iter().filter(|m| !m.is_empty()));
                grown = true;
            }
            Some(Err(())) => {
                unread.push(ex.len());
                ex.push(w.to_string());
            }
            None => ex.push(w.to_string()),
        }
    }
    let lead = |n: &str| WRAPPERS.contains(&n) || runner(n);
    const STEERS: [&str; 8] = ["git", "gh", "sh", "bash", "zsh", "eval", "source", "."];
    // A read-only git subcommand right after `git`: an unread word there writes no ref.
    const RO_GIT: [&str; 8] = [
        "log",
        "show",
        "diff",
        "status",
        "grep",
        "blame",
        "rev-parse",
        "ls-files",
    ];
    for &at in &unread {
        let assign = |w: &String| w.contains('=') && !w.starts_with('-');
        let ro_git = !ex.iter().any(|w| name(w) == "gh")
            && ex.iter().position(|w| name(w) == "git").is_some_and(|p| {
                ex[..p].iter().all(assign)
                    && at > p + 1
                    && ex.get(p + 1).is_some_and(|s| RO_GIT.contains(&s.as_str()))
                    // No hidden option (`--output=`, `grep -O`) can sit in the unread word.
                    && plain_braces(&ex[at])
            });
        let refuse = ex[..at].iter().any(|w| lead(&name(w)))
            || if ex[..at].iter().all(assign) {
                first_word(&ex[at]).is_none_or(|f| lead(&name(&f)) || STEERS.contains(&&*name(&f)))
            } else {
                !ro_git && ex.iter().any(|w| matches!(name(w).as_str(), "git" | "gh"))
            };
        if refuse {
            return Some(UNREAD_BRACE);
        }
    }
    if grown {
        let words: Vec<&str> = ex.iter().map(String::as_str).collect();
        return merge_rule(&words, &ex.join(" "), target);
    }
    let (mut words, mut xargs, mut moved) = (words, false, false);
    loop {
        let skip = words
            .iter()
            .take_while(|w| {
                (w.contains('=') && !w.starts_with('-')) || (WRAPPERS.contains(w) && !runner(w))
            })
            .count();
        let (cmd, dirs, _) = unwrap_runners(&words[skip..]);
        let ran = &words[skip..words.len() - cmd.len()];
        xargs |= ran.iter().any(|w| w.rsplit('/').next() == Some("xargs"));
        moved |= !dirs.is_empty();
        if skip == 0 && ran.is_empty() {
            break;
        }
        words = cmd;
    }
    // A runner that changes directory moves git to a place the guard cannot read: fail closed.
    let target = Target {
        dir: target.dir.filter(|_| !moved),
        ..*target
    };
    if xargs {
        let blind = match words.first().copied() {
            Some("gh") => {
                words.len() < 3 || matches!(words[1], "api" | "release" | "alias" | "repo")
            }
            Some("git") => words.len() < 2 || words[1..].iter().any(|w| XARGS_GIT.contains(w)),
            Some("curl" | "wget" | "http" | "https" | "xh") => true,
            _ => false,
        };
        if blind {
            return Some("`xargs` adds arguments the guard cannot see");
        }
    }
    // `\git`, `"git"`, `g''it` and `/usr/bin/git` all run git.
    // A word with an open quote is part of a longer string (`python -c 'gh ..'`): left as is.
    let first = words.first().map(|w| {
        let even = |q: char| w.matches(q).count() % 2 == 0;
        if even('\'') && even('"') {
            w.replace(['\'', '"', '\\'], "")
        } else {
            w.to_string()
        }
    });
    let first = first.as_deref().map(|w| w.rsplit('/').next().unwrap_or(w));
    if let Some(why) = exec_rule(first, words, segment, &target) {
        return Some(why);
    }
    // `git-fetch`, `/usr/lib/git-core/git-push`: a git-core program runs that subcommand.
    if let Some(sub) = first.and_then(|f| f.strip_prefix("git-")) {
        let args: Vec<&str> = std::iter::once(sub)
            .chain(words[1..].iter().copied())
            .collect();
        return git_rule(&args, &target);
    }
    match first {
        Some("gh") => gh_rule(&words[1..], segment),
        // `GIT_CONFIG_KEY_n`, `GIT_CONFIG_PARAMETERS`: config from the environment.
        Some("git")
            if segment.contains("GIT_CONFIG_")
                && segment.to_ascii_lowercase().contains("alias.") =>
        {
            Some(GIT_ALIAS)
        }
        Some("git")
            if segment.contains("GIT_CONFIG_")
                && segment.to_ascii_lowercase().contains(".fetch") =>
        {
            Some(FETCH_MOVES)
        }
        Some("git") => git_rule(&words[1..], &target),
        Some("curl" | "wget" | "http" | "https" | "xh") => http_rule(segment),
        _ => None,
    }
}

/// A brace word of letters, digits, `{}`, `,` and `.` whose `x..y` ranges join two digits or two
/// letters of one case: it can hold no option (`{!..~}` and `{0..z}` span `-` and `=`).
fn plain_braces(w: &str) -> bool {
    let same = |a: char, b: char| {
        (a.is_ascii_digit() && b.is_ascii_digit())
            || (a.is_ascii_lowercase() && b.is_ascii_lowercase())
            || (a.is_ascii_uppercase() && b.is_ascii_uppercase())
    };
    w.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '{' | '}' | ',' | '.'))
        && w.match_indices("..").all(|(i, _)| {
            match (w[..i].chars().last(), w[i + 2..].chars().next()) {
                (Some(a), Some(b)) => same(a, b),
                _ => false,
            }
        })
}

/// Commands that run another command: `find -exec`, `fd -x`, and `awk system()`.
fn exec_rule(
    first: Option<&str>,
    words: &[&str],
    segment: &str,
    target: &Target,
) -> Option<&'static str> {
    match first {
        Some("find" | "fd" | "fdfind") => {
            let at = words.iter().position(|w| {
                matches!(
                    *w,
                    "-exec"
                        | "-execdir"
                        | "-ok"
                        | "-okdir"
                        | "-x"
                        | "-X"
                        | "--exec"
                        | "--exec-batch"
                )
            })?;
            let end = words[at + 1..]
                .iter()
                .position(|w| matches!(*w, ";" | "+" | "\\;"))
                .map_or(words.len(), |e| at + 1 + e);
            let inner = &words[at + 1..end];
            // `{}` stands for names find adds: judged like the command without them.
            merge_rule(inner, &inner.join(" "), target)
        }
        Some("awk" | "gawk" | "mawk" | "nawk") => {
            calls_system(segment).then_some("`awk` runs commands through system()")
        }
        _ => None,
    }
}

/// Whether the text has `system` followed by optional blanks and `(`: an awk `system()` call.
fn calls_system(s: &str) -> bool {
    s.match_indices("system")
        .any(|(i, m)| s[i + m.len()..].trim_start().starts_with('('))
}

const UNREAD_BRACE: &str = "a `{x..y}` brace sequence the guard cannot see through: \
    write the words out";

/// Bash brace expansion of one word: `None` when it has no unquoted `{a,b}` list or `{x..y}`
/// sequence (`${x}`, `@{u}`, `{}` stay as they are); `Err` for a sequence bash would not expand,
/// more than 64 words or more than 8 lists in one word. The scan starts at `from`, where no
/// quote is open (all before it is already expanded).
fn braces(word: &str, from: usize, depth: usize) -> Option<Result<Vec<String>, ()>> {
    // Each open `{` scans to the end of the word: a long one is not read (fail closed).
    if word.len() > 1024 && word.contains('{') {
        return Some(Err(()));
    }
    let mut at = from;
    while let Some(i) = open_brace(word, at) {
        at = i + 1;
        // An open `{` with a list after it: a quote split it from its `}` (`{a,'b c'}`).
        let Some((end, cuts)) = brace_list(&word[i..]) else {
            if word[i..].contains(',') || word[i..].contains("..") {
                return Some(Err(()));
            }
            continue;
        };
        let alts: Vec<String> = if cuts.is_empty() {
            if !word[i..i + end].contains("..") {
                continue;
            }
            match sequence(&word[i + 1..i + end]) {
                Some(items) => items,
                None => return Some(Err(())),
            }
        } else {
            let starts = [0].into_iter().chain(cuts.iter().map(|c| c + 1));
            let ends = cuts.iter().copied().chain([end]);
            starts
                .zip(ends)
                .map(|(a, b)| word[i + a.max(1)..i + b].to_string())
                .collect()
        };
        if alts.len() > 64 || depth >= 8 {
            return Some(Err(()));
        }
        let (head, tail) = (&word[..i], &word[i + end + 1..]);
        let mut out = Vec::new();
        for alt in alts {
            let one = format!("{head}{alt}{tail}");
            match braces(&one, i, depth + 1) {
                None => out.push(one),
                Some(Ok(more)) => out.extend(more),
                Some(Err(())) => return Some(Err(())),
            }
            if out.len() > 64 {
                return Some(Err(()));
            }
        }
        return Some(Ok(out));
    }
    None
}

/// The offset of the first `{` at or after `from` outside quotes and escapes, not after `$`.
fn open_brace(word: &str, from: usize) -> Option<usize> {
    let (mut quote, mut esc) = (None, false);
    for (i, c) in word[from..].char_indices().map(|(i, c)| (i + from, c)) {
        match (quote, c) {
            _ if std::mem::take(&mut esc) => {}
            (Some('\''), '\'') => quote = None,
            (Some('\''), _) => {}
            (_, '\\') => esc = true,
            (Some(_), '"') => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '{') if !word[..i].ends_with('$') => return Some(i),
            _ => {}
        }
    }
    None
}

/// The first word bash makes of `word`, which runs when it is the command word: each list or
/// sequence gives its first item, and a `{` a quote split from its `}` ends at its first `,`.
/// `None` when that item is empty (bash drops an empty word, so the next one runs).
fn first_word(word: &str) -> Option<String> {
    let (mut w, mut at) = (word.to_string(), 0);
    for _ in 0..64 {
        let Some(i) = open_brace(&w, at) else {
            return Some(w);
        };
        at = i + 1;
        let (end, item) = match brace_list(&w[i..]) {
            Some((end, cuts)) if !cuts.is_empty() => (end, w[i + 1..i + cuts[0]].to_string()),
            Some((end, _)) => match sequence(&w[i + 1..i + end]) {
                Some(items) => (end, items[0].clone()),
                None => continue,
            },
            None => {
                let item = w[i + 1..].split([',', ' ']).next().unwrap_or_default();
                return Some(format!("{}{item}", &w[..i])).filter(|f| !f.is_empty());
            }
        };
        (!item.is_empty()).then_some(())?;
        w = format!("{}{item}{}", &w[..i], &w[i + end + 1..]);
        at = i;
    }
    None
}

/// The words of a bash `{x..y[..step]}` sequence (integers or single letters), at most 64.
fn sequence(inner: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = inner.split("..").collect();
    let step = match parts.get(2) {
        Some(s) => s.parse::<i64>().ok()?.unsigned_abs().max(1),
        None => 1,
    };
    let (a, b) = match parts[..] {
        [a, b] | [a, b, _] => (a, b),
        _ => return None,
    };
    let num = |x: &str| x.parse::<i64>().ok();
    let letter = |x: &str| {
        (x.len() == 1 && x.as_bytes()[0].is_ascii_alphabetic()).then(|| i64::from(x.as_bytes()[0]))
    };
    let (lo, hi, text) = match (num(a), num(b), letter(a), letter(b)) {
        (Some(x), Some(y), ..) => (x, y, false),
        (_, _, Some(x), Some(y)) => (x, y, true),
        _ => return None,
    };
    // A counted loop in i128: no bound or step near the i64 limits can overflow or spin.
    let count = lo.abs_diff(hi) / step;
    if count >= 64 {
        return None;
    }
    let pad = [a, b]
        .iter()
        .any(|x| x.trim_start_matches('-').len() > 1 && x.trim_start_matches('-').starts_with('0'));
    let width = if pad { a.len().max(b.len()) } else { 0 };
    let dir: i128 = if lo <= hi { 1 } else { -1 };
    let out = (0..=count)
        .map(|k| {
            let v = i128::from(lo) + dir * i128::from(k) * i128::from(step);
            match u8::try_from(v) {
                Ok(c) if text => char::from(c).to_string(),
                _ => format!("{v:0width$}"),
            }
        })
        .collect();
    Some(out)
}

/// For text starting with `{`: the offset of its matching `}` and of its top-level commas.
fn brace_list(s: &str) -> Option<(usize, Vec<usize>)> {
    let (mut depth, mut quote, mut esc, mut cuts) = (0, None, false, Vec::new());
    for (i, c) in s.char_indices() {
        match (quote, c) {
            _ if std::mem::take(&mut esc) => {}
            (Some('\''), '\'') => quote = None,
            (Some('\''), _) => {}
            (_, '\\') => esc = true,
            (Some(_), '"') => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') => {
                depth -= 1;
                if depth == 0 {
                    return Some((i, cuts));
                }
            }
            (None, ',') if depth == 1 => cuts.push(i),
            _ => {}
        }
    }
    None
}

/// For a command word the shell builds (`$g`, `"$G"`, `${X:-y}`, `$(..)`, a backtick span): the
/// name (`gh`, `git`) that, put in its place, makes the segment a merge, tag or push. `None` when
/// neither does, as for `$HOME/.cargo/bin/cargo test`.
fn built_name_rule(word: &str, segment: &str, target: &Target) -> Option<&'static str> {
    let start = word.as_ptr() as usize - segment.as_ptr() as usize;
    let rest = &segment[start..];
    // The word ends at the first blank outside quotes, `( )`, `{ }` and backtick spans.
    let (mut depth, mut quote, mut tick) = (0i32, None::<char>, false);
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '`') => tick = !tick,
            (None, '(' | '{') => depth += 1,
            (None, ')' | '}') => depth -= 1,
            (None, c) if c.is_whitespace() && depth <= 0 && !tick => {
                end = i;
                break;
            }
            _ => {}
        }
    }
    ["gh", "git"].into_iter().find(|name| {
        let swapped = format!("{name}{}", &rest[end..]);
        let words: Vec<&str> = swapped.split_whitespace().collect();
        merge_rule(&words, &swapped, target).is_some()
    })
}

/// A shell or interpreter that reads its script from the pipe it is fed: `curl .. | sh`.
fn reads_pipe(words: &[&str]) -> bool {
    let (cmd, _, _) = unwrap_runners(words);
    let name = cmd.first().map_or("", |w| base(w));
    let name = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    if ![
        "sh", "bash", "zsh", "dash", "ksh", "fish", "python", "perl", "ruby", "node",
    ]
    .contains(&name)
    {
        return false;
    }
    let args = &cmd[1..];
    args.contains(&"-s")
        || args.iter().all(|a| a.starts_with('-') && *a != "-c")
        || args.contains(&"-")
}

fn destructive(what: &str) -> String {
    format!("destructive-lock: `{what}` can wipe a machine or a database; ask the owner to run it")
}

fn base(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// A client whose arguments, stdin or heredoc are SQL.
fn sql_client(word: &str) -> bool {
    let clients = "psql pgcli mysql mariadb mycli sqlite3 duckdb usql cockroach clickhouse sqlcmd \
        clickhouse-client";
    clients.split_whitespace().any(|c| base(word) == c)
}

/// Destructive-lock (spec §4): the command word of every segment after wrappers and runners,
/// and of every nested command; quoted text, other arguments and heredoc bodies are data.
fn destructive_rule(command: &str, depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return Some(too_deep());
    }
    let (code, subs) = split_heredocs(command);
    let inner = subs.into_iter().chain(substitutions(&code, false));
    if let Some(why) = inner
        .chain(word_scripts(&code))
        .find_map(|s| destructive_rule(&s, depth + 1))
    {
        return Some(why);
    }
    let words = shell_words(&code);
    // The fork bomb `:(){ :|:& };:` as words (quoted, it is one word: data).
    if words.concat().contains(":(){:\0:\0}\0:") {
        return Some(destructive("fork bomb"));
    }
    let segments: Vec<&[String]> = words
        .split(|w| w == SEP)
        .filter(|s| !s.is_empty())
        .collect();
    let mut client = None;
    for (i, segment) in segments.iter().enumerate() {
        if disk_redirect(segment) {
            return Some(destructive("redirect into a disk device"));
        }
        let all: Vec<&str> = segment.iter().map(String::as_str).collect();
        let mut cmd = all.as_slice();
        loop {
            let runs = |w: &str| RUNNERS.iter().any(|r| r.0 == base(w));
            let skip = (cmd.iter())
                .take_while(|w| assigned(w).is_some() || WRAPPERS.contains(w) && !runs(w))
                .count();
            let (rest, _, shell) = unwrap_runners(&cmd[skip..]);
            if shell && let Some(why) = destructive_rule(&rest.join(" "), depth + 1) {
                return Some(why);
            }
            if rest.len() == cmd.len() {
                break;
            }
            cmd = rest;
        }
        let (name, args) = cmd.split_first().unwrap_or((&"", &[]));
        if wipes(base(name), args) {
            return Some(destructive(&segment.join(" ")));
        }
        // SQL is what a client reads: its arguments, a pipe into it, its heredoc.
        let remote = ["docker", "podman", "kubectl", "ssh"].contains(&base(name));
        let mut named = cmd.iter().take(if remote { cmd.len() } else { 1 });
        if named.any(|w| sql_client(w)) {
            client.get_or_insert(i);
        }
    }
    let sql = segments[client?.saturating_sub(1)..].concat().join(" ");
    let sql: Vec<&str> = sql.split_whitespace().collect();
    let sql = sql.join(" ").to_ascii_lowercase();
    ["drop database", "drop schema"]
        .into_iter()
        .find(|d| sql.contains(d))
        .map(destructive)
}

/// A redirect (`>`, `>>`, `&>`, `2>|`, glued or not) into a disk device.
fn disk_redirect(segment: &[String]) -> bool {
    segment.iter().enumerate().any(|(i, w)| {
        let w = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        let glued = w.trim_start_matches(['>', '|', '&']);
        let next = segment.get(i + 1).map_or("", String::as_str);
        let dev = (if glued.is_empty() { next } else { glued }).strip_prefix("/dev/");
        w.starts_with('>') && dev.is_some_and(|d| DISKS.iter().any(|k| d.starts_with(k)))
    })
}

const DISKS: &[&str] = &["sd", "nvme", "disk", "hd", "vd"];

/// `/`, `~` or `$HOME` (in any spelling) alone or with `/`, `/*` or `/.`.
fn wipe_target(a: &str) -> bool {
    let home = ["~", "$HOME", "${HOME}", "${HOME:?}"]
        .iter()
        .find_map(|h| a.strip_prefix(h));
    ["/", "/*", "/."].contains(&a) || home.is_some_and(|r| ["", "/", "/*", "/."].contains(&r))
}

/// Whether the command `name ARGS` wipes a machine, a disk or an infrastructure.
fn wipes(name: &str, args: &[&str]) -> bool {
    let flag = |long: &str, short: &[char]| {
        let short = |a: &&str| a.starts_with('-') && !a.starts_with("--") && a.contains(short);
        args.iter().any(|a| *a == long || short(a))
    };
    let aimed = args.iter().any(|a| wipe_target(a));
    match name {
        "rm" => {
            args.contains(&"--no-preserve-root")
                || (flag("--recursive", &['r', 'R']) || flag("--force", &['f'])) && aimed
        }
        "chmod" | "chown" | "chgrp" => flag("--recursive", &['R']) && aimed,
        "dd" => (args.iter().filter_map(|a| a.strip_prefix("of=/dev/")))
            .any(|t| !["null", "stdout", "stderr", "tty"].contains(&t) && !t.starts_with("fd/")),
        "shred" | "wipefs" => args.iter().any(|a| a.starts_with("/dev/")),
        "terraform" | "tofu" => match args.iter().find(|a| !a.starts_with('-')) {
            Some(&"destroy") => true,
            Some(&"apply") => args.iter().any(|a| a.trim_start_matches('-') == "destroy"),
            _ => false,
        },
        n => n == "mke2fs" || n.starts_with("mkfs"),
    }
}

/// The command without its heredoc bodies (data, unless a shell or a SQL client on the opening
/// line reads it), and the substitutions in unquoted bodies (they run).
fn split_heredocs(command: &str) -> (String, Vec<String>) {
    let (mut code, mut subs) = (String::new(), Vec::new());
    let mut skip: Option<(String, bool, bool)> = None;
    for line in command.split_inclusive('\n') {
        if let Some((delim, dash, quoted)) = &skip {
            let l = line.trim_end_matches('\n');
            let l = if *dash { l.trim_start_matches('\t') } else { l };
            // Bash ends a heredoc inside `$( )` at `EOF)`: a body line that starts with the
            // delimiter and more is not plain data; fail closed and judge the body as code.
            if l != delim && l.starts_with(delim.as_str()) {
                return (command.to_string(), Vec::new());
            }
            if l == delim {
                skip = None;
                code.push_str(line);
            } else if !quoted {
                subs.extend(substitutions(line, true));
            }
            continue;
        }
        code.push_str(line);
        let Some(at) = heredoc_at(line) else { continue };
        let dash = line[at + 2..].starts_with('-');
        let spec = line[at + 2..].trim_start_matches('-').trim_start();
        let quote = spec.chars().next().filter(|c| matches!(c, '\'' | '"'));
        let delim = match quote {
            Some(q) => spec[1..].find(q).map(|end| spec[1..=end].to_string()),
            None => Some(
                (spec.chars())
                    .take_while(|c| !c.is_whitespace() && !";|&<>()".contains(*c))
                    .filter(|c| *c != '\\')
                    .collect(),
            ),
        };
        let reads = shell_words(line)
            .iter()
            .any(|w| SHELLS.contains(&base(w)) || interp::is_runner(w) || sql_client(w));
        if let Some(delim) = delim.filter(|d| !d.is_empty() && !reads) {
            skip = Some((delim, dash, quote.is_some()));
        }
    }
    (code, subs)
}

/// The `$( )`, backtick and (outside quotes) `( )` spans of a command line, quote-aware; in a
/// heredoc `body` quotes are text and only `$( )` and backticks run.
fn substitutions(text: &str, body: bool) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let (mut out, mut quote, mut i) = (Vec::new(), None, 0);
    while i < c.len() {
        match (quote, c[i]) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => {}
            (_, '\\') => i += 1,
            (None, '\'' | '"') if !body => quote = Some(c[i]),
            (_, '`') => {
                let end = (i + 1..c.len()).find(|&j| c[j] == '`' && c[j - 1] != '\\');
                let end = end.unwrap_or(c.len());
                out.push(c[i + 1..end].iter().collect());
                i = end;
            }
            (q, '(') if (q.is_none() && !body) || (i > 0 && c[i - 1] == '$') => {
                let (mut level, mut j) = (1, i + 1);
                while j < c.len() && level > 0 {
                    level += i32::from(c[j] == '(') - i32::from(c[j] == ')');
                    j += 1;
                }
                let end = if level == 0 { j - 1 } else { j };
                out.push(c[i + 1..end].iter().collect());
                i = j;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

const DECRYPT_FIX: &str = "secrets stay encrypted; the owner runs `hoist secrets edit`, CI \
    decrypts with `hoist secrets exec` (spec §8)";

fn decrypt_reason(what: &str) -> String {
    format!("decrypting secrets is refused for agents ({what}): {DECRYPT_FIX}")
}

/// An environment dump or a read of the process environment: it can hold tokens and keys, but it
/// is not a decrypt, so it gets its own reason.
fn env_reason(what: &str) -> String {
    format!(
        "dumping {what} is refused for agents (it can hold tokens and keys): read only the \
         variables you need, by name (spec §8)"
    )
}

const HOIST_SECRETS: &[&str] = &["edit", "sync", "grant", "revoke", "rotate", "rotate-key"];
/// Readers: their arguments are data (patterns, text).
#[rustfmt::skip]
const READERS: &[&str] = &["grep", "egrep", "fgrep", "rgrep", "zgrep", "ugrep", "ug", "ugrep+",
    "ug+", "pcregrep", "pcre2grep", "rg", "ag", "ack", "ack-grep", "cat", "head", "tail", "ls", "wc",
    "echo", "printf", "which", "type"];
/// The grep family; rg, ag, ack and rgrep always recurse, rg and ag skip hidden directories.
#[rustfmt::skip]
const SEARCHERS: &[&str] = &["grep", "egrep", "fgrep", "rgrep", "zgrep", "ugrep", "ug", "ugrep+",
    "ug+", "pcregrep", "pcre2grep", "rg", "ag", "ack", "ack-grep"];
/// Searchers whose options and environment run commands: only their pattern is data.
const PAGED: &[&str] = &["ack", "ack-grep", "ugrep", "ug", "ugrep+", "ug+"];
#[rustfmt::skip]
const ENV_READS: &[&str] = &["environ", "process.env", "ENV[", "ENV.", "ENV{", "ENVIRON[", "getenv",
    "Deno.env", "Bun.env", "%ENV", ").env", "$_ENV", "$_SERVER"];
const INTERPS: &[&str] = &[
    "python", "python3", "perl", "ruby", "node", "deno", "bun", "php",
];
const FILE_TOOLS: &[&str] = &[
    "mv", "cp", "install", "chmod", "chown", "ln", "mkdir", "rmdir", "rm", "touch", "tar", "unzip",
    "gzip", "gunzip", "curl", "wget", "stat", "file", "du", "df",
];
/// Package managers, and the subcommands whose arguments are package names (never `run`/`exec`).
const PACKAGERS: &[&str] = &[
    "brew", "apt", "apt-get", "dnf", "yum", "apk", "pacman", "port", "pip", "pip3", "pipx", "npm",
    "pnpm", "yarn", "bun", "cargo",
];
#[rustfmt::skip]
const PKG_SUBS: &[&str] = &["install", "add", "remove", "uninstall", "upgrade", "update", "info",
    "search", "show", "list", "build", "test", "nextest", "check", "clippy", "fmt"];
#[rustfmt::skip]
const GH_DATA: &[&str] = &["t", "title", "b", "body", "notes", "f", "F", "raw-field", "field",
    "search", "q", "jq"];
/// The pagers ack and ugrep may run, and the editors ugrep `--view` may run with no arguments.
const PAGERS_OK: &[&str] = &["less", "more", "most", "cat"];
const VIEWERS_OK: &[&str] = &["vi", "vim", "nvim", "view", "nano", "emacs", "code"];
const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "ksh", "fish", "busybox", "python", "python3", "perl", "ruby",
    "node", "deno", "bun", "php", "source", "xargs", "eval",
];

/// The files that `> f`, `>> f` and `tee f` write in these words.
fn write_targets(w: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, x) in w.iter().enumerate() {
        let r = x.trim_start_matches('>');
        #[rustfmt::skip]
        let t = if x.starts_with('>') { if r.is_empty() { w.get(i + 1).map(String::as_str) } else { Some(r) } }
            else if x == "tee" { w[i + 1..].iter().map(String::as_str).find(|a| !a.starts_with('-')) } else { None };
        out.extend(t.map(str::to_string));
    }
    out
}

/// An age secret key line (or its tail) named in a word, in any case.
fn keyline(w: &str) -> bool {
    let w = w.to_lowercase();
    w.contains("age-secret") || w.contains("secret-key-1")
}

/// A word naming `.config/sops` as a whole path segment (`.config/sops-test` is another dir).
fn sops_dir(w: &str) -> bool {
    w.match_indices(".config/sops")
        .any(|(i, m)| w[i + m.len()..].is_empty() || w[i + m.len()..].starts_with('/'))
}

/// A path with `//`, `.` and `dir/..` collapsed, lexically (no fs). `..` stays after `~`, `$X`,
/// a glob or another `..`, and anywhere under `/proc` (its links are symlinks); `/..` is `/`.
fn lexical(path: &str) -> String {
    let (abs, proc) = (path.starts_with('/'), path.starts_with("/proc/"));
    let mut out: Vec<&str> = Vec::new();
    for s in path.split('/') {
        #[rustfmt::skip]
        let plain = |l: &&str| !proc && *l != ".." && !l.starts_with(['~', '$']) && !l.contains(['*', '?', '[']);
        match s {
            "" | "." => {}
            ".." if out.last().is_some_and(plain) => drop(out.pop()),
            ".." if abs && out.is_empty() => {}
            _ => out.push(s),
        }
    }
    let j = out.join("/");
    match (abs, j.is_empty() && !path.is_empty()) {
        (true, _) => format!("/{j}"),
        (false, true) => ".".to_string(),
        _ => j,
    }
}

/// How much of the age key directory a search root contains: 2 for `.config` or an ancestor
/// of `.config/sops` inside it (`~/.config`, `$XDG_CONFIG_HOME`), or a pure `..` chain above
/// an unknown place (`..`, `$PWD/..`) or a glob there (`/*`, `../*`); 1 for home or above it
/// (`~`, `~user`, `/root`, `/home/<user>`, `/home`, `/`, `../bob`, `../home/bob`); 0 for
/// anything else (`~/projects/x`, `../x/src`).
fn key_root(word: &str) -> u8 {
    let n = lexical(word.trim_matches(|c| c == '"' || c == '\''));
    let word = n.as_str();
    let t = word.trim_end_matches('/');
    // A `..` left after `lexical` climbs from an unknown place: what follows the last one is
    // judged as under `/` and as under a home (`..` alone is above it, `../bob` may be a home).
    let segs: Vec<&str> = t.split('/').collect();
    if let Some(i) = segs.iter().rposition(|s| *s == "..") {
        let tail = &segs[i + 1..];
        #[rustfmt::skip]
        let key = tail.iter().any(|s| matches!(*s, ".config" | "sops" | "age") || s.contains(['*', '?', '[']));
        let j = tail.join("/");
        let as_root = key_root(&format!("/{j}")).max(key_root(&format!("~/{j}")));
        return if tail.is_empty() || key {
            2
        } else {
            as_root.max(u8::from(tail.len() == 1))
        };
    }
    let odd = t.contains(['*', '?', '[']);
    // A glob in a segment of home or above it may match the key directory (`/*`, `/r*`).
    if let Some(g) = t.find(['*', '?', '[']) {
        let dir = t[..g]
            .rfind('/')
            .map_or("", |i| if i == 0 { "/" } else { &t[..i] });
        if !dir.is_empty() && key_root(dir) == 1 {
            return 2;
        }
    }
    let under = |r: &str, own: &str| {
        r == own
            || r.strip_prefix(own)
                .is_some_and(|x| x == "/sops" || x.starts_with("/sops/"))
    };
    if t.is_empty() || t == "/home" {
        return u8::from(word.starts_with('/') || word.is_empty());
    }
    if let Some(r) = ["$XDG_CONFIG_HOME", "${XDG_CONFIG_HOME}"]
        .iter()
        .find_map(|h| t.strip_prefix(h))
    {
        return if odd || r.is_empty() || r.starts_with("/sops/") || r == "/sops" {
            2
        } else {
            0
        };
    }
    let rest = ["$HOME", "${HOME}", "/root"]
        .iter()
        .find_map(|h| t.strip_prefix(h))
        .or_else(|| {
            let user = t.strip_prefix('~')?;
            Some(user.find('/').map_or("", |i| &user[i..]))
        })
        .or_else(|| {
            let user = t.strip_prefix("/home/")?;
            Some(user.find('/').map_or("", |i| &user[i..]))
        });
    match rest {
        Some(_) if odd => 2,
        Some("") => 1,
        Some(r) if under(r, "/.config") => 2,
        None if under(t.trim_start_matches("./"), ".config") => 2,
        _ => 0,
    }
}

/// Whether a search of these roots reads the age key: one is under `.config/sops`, or the
/// search recurses into `.config` or an ancestor (home or above only when it enters hidden dirs).
fn key_search(roots: &[&str], recursive: bool, hidden: bool) -> bool {
    let root = roots.iter().map(|a| key_root(a)).max().unwrap_or(0);
    roots.iter().any(|a| sops_dir(a)) || recursive && (root == 2 || root == 1 && hidden)
}

/// A redirect word: `>`, `>>`, `2>`, `&>`, `<`, `2>&1`, `>file`.
fn redirect(a: &str) -> bool {
    a.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&')
        .starts_with(['>', '<'])
}

fn procenv(w: &str) -> bool {
    w.contains("/proc/") && w.contains("env")
}

/// Whether an interpreter or shell command line has code on it (`-c`, `-e`, `--eval`, `eval`, `php -r`).
fn runs_code(n: &str, after: &[&str]) -> bool {
    let dash = |a: &&str, c: char| a.starts_with('-') && !a.starts_with("--") && a.contains(c);
    let interp = INTERPS.contains(&n);
    #[rustfmt::skip]
    let code = |a: &&str| dash(a, 'c') || n == "bun" && (dash(a, 'p') || *a == "--print") || interp && (dash(a, 'e') || *a == "--eval" || *a == "eval" || n == "php" && dash(a, 'r'));
    after.iter().any(code)
}

/// A flag without its dashes and value: `--decrypt=x` and `-decrypt` are `decrypt`.
fn flag(arg: &str) -> Option<&str> {
    let f = arg.strip_prefix('-')?;
    Some(f.trim_start_matches('-').split('=').next().unwrap_or(""))
}

/// A long option given as `full` or a prefix of it at least `min` long (git, getopt and ack
/// take unique prefixes; `min` is the shortest one unique among the tool's options).
fn long_prefix(arg: &str, full: &str, min: usize) -> bool {
    let n = arg.split('=').next().unwrap_or(arg);
    n.len() >= min && full.starts_with(n)
}

/// The values of a long option or its prefixes: `--opt=v`, else the next word.
fn long_values<'a>(args: &[&'a str], full: &str, min: usize) -> Vec<&'a str> {
    #[rustfmt::skip]
    let v = args.iter().enumerate().filter(|(_, a)| long_prefix(a, full, min))
        .filter_map(|(i, a)| a.split_once('=').map(|p| p.1).or_else(|| args.get(i + 1).copied()));
    v.collect()
}

/// Whether the commands ack and ugrep run on their output or files are all known pagers:
/// `--pager` and ugrep's `--view` (also abbreviated), `ACK_PAGER`, `ACK_PAGER_COLOR` and the
/// options in `ACK_OPTIONS` and `PAGER` (set anywhere in the command), and ugrep's view
/// fallbacks `EDITOR` and `VISUAL`; `LESS` may hold only such flags, and `LESSOPEN`,
/// `LESSCLOSE`, `LESSKEY*`, `LESSSECURE`, `BAT_PAGER` and vim's or nvim's init variables may not
/// be set. ugrep's `--pager` and `--view` take a value only with `=`. A pager is a known name, bare or in `/usr/bin` or `/bin`, with plain flag clusters
/// (`-R`, `-`; no `k`, `o` or `O`, which read keys or write files); an editor may only view,
/// with no arguments.
fn pagers_ok(name: &str, words: &[String], args: &[&str]) -> bool {
    let mut opts: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut cmds: Vec<(String, bool)> = Vec::new();
    let ugrep = name.starts_with('u');
    // ugrep opens a viewer only from its query TUI: `-Q` (also in a cluster), `--query`, `--view`.
    #[rustfmt::skip]
    let views = ugrep && args.iter().any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains('Q') || long_prefix(a, "--query", 5) || long_prefix(a, "--view", 4));
    for (k, v) in words
        .iter()
        .filter_map(|w| Some((assigned(w)?, w.split_once('=')?.1)))
    {
        match k {
            "ACK_PAGER" | "ACK_PAGER_COLOR" | "PAGER" => cmds.push((v.to_string(), false)),
            "EDITOR" | "VISUAL" if views => cmds.push((v.to_string(), true)),
            "ACK_OPTIONS" => opts.extend(shell_words(v)),
            "LESS" if shell_words(v).iter().all(|a| pager_flag(a)) => {}
            "LESS" | "LESSOPEN" | "LESSCLOSE" | "LESSKEY" | "LESSKEYIN" | "LESSKEY_SYSTEM"
            | "LESSSECURE" | "BAT_PAGER" => return false,
            // Vim and nvim init commands and files (`init.lua` under `XDG_CONFIG_HOME`).
            "VIMINIT" | "EXINIT" | "MYVIMRC" | "GVIMINIT" | "VIM" | "VIMRUNTIME"
            | "XDG_CONFIG_HOME" | "NVIM_APPNAME"
                if views =>
            {
                return false;
            }
            _ => {}
        }
    }
    let opts: Vec<&str> = opts.iter().map(String::as_str).collect();
    // ugrep's values are optional: a bare `--pager` or `--view` uses the default.
    let eq = |full: &str, min: usize| -> Vec<&str> {
        #[rustfmt::skip]
        let v = opts.iter().filter(|a| long_prefix(a, full, min)).filter_map(|a| Some(a.split_once('=')?.1));
        v.collect()
    };
    let (view, pager) = match ugrep {
        true => (eq("--view", 4), eq("--pager", 5)),
        false => (Vec::new(), long_values(&opts, "--pager", 5)),
    };
    #[rustfmt::skip]
    cmds.extend(pager.into_iter().map(|c| (c.to_string(), false))
        .chain(view.into_iter().map(|c| (c.to_string(), true))));
    cmds.iter().all(|(c, view)| {
        let w = shell_words(c);
        let Some((cmd, rest)) = w.split_first() else {
            return false;
        };
        #[rustfmt::skip]
        let cmd = ["/usr/bin/", "/bin/"].iter().find_map(|d| cmd.strip_prefix(d)).unwrap_or(cmd);
        let pager = PAGERS_OK.contains(&cmd) && rest.iter().all(|a| pager_flag(a));
        let editor = *view && VIEWERS_OK.contains(&cmd) && rest.is_empty();
        !c.contains(['|', ';', '&', '<', '>', '(', ')', '`', '$', '\n']) && (pager || editor)
    })
}

/// A plain pager flag cluster (`-R`, `-` alone), without less's `k`, `o` or `O`.
fn pager_flag(a: &str) -> bool {
    let f = |c: char| c.is_ascii_alphabetic() && !matches!(c, 'k' | 'o' | 'O');
    a == "-" || a.len() > 1 && a.starts_with('-') && a[1..].chars().all(f)
}

/// `sops` may print info or encrypt; `sops FILE` decrypts, so any other argument is refused.
fn sops_ok(args: &[&str]) -> bool {
    const INFO: &[&str] = &["version", "v", "help", "h"];
    #[rustfmt::skip]
    const BAD: &[&str] = &["d", "decrypt", "edit", "exec-env", "exec-file", "set", "rotate", "r",
        "updatekeys", "keyservice", "unset", "publish", "filestatus", "groups"];
    let info = |a: &&str| flag(a).is_some_and(|f| INFO.contains(&f));
    // Flags that take the next word as their value, and boolean ones. The first bare word or
    // unknown flag ends the walk: `-e` after it, or as a value (`--age -e`), is not a marker.
    #[rustfmt::skip]
    const VALUE: &[&str] = &["config", "age", "pgp", "kms", "gcp-kms", "azure-kv", "hc-vault-transit",
        "input-type", "output-type", "unencrypted-suffix", "encrypted-suffix", "unencrypted-regex",
        "encrypted-regex", "output", "indent", "filename-override", "shamir-secret-sharing-threshold"];
    let (mut it, mut enc) = (args.iter(), false);
    while let Some(a) = it.next() {
        let f = flag(a);
        enc = *a == "encrypt" || matches!(f, Some("e" | "encrypt"));
        match f {
            _ if enc => break,
            Some(f)
                if a.contains('=') || matches!(f, "i" | "in-place" | "verbose" | "ignore-mac") => {}
            Some(f) if VALUE.contains(&f) => drop(it.next()),
            _ => break,
        }
    }
    let bad = |a: &&str| BAD.contains(&flag(a).unwrap_or(a));
    args.is_empty() || args.iter().all(info) || (enc && !args.iter().any(bad))
}

/// `age`/`rage` may print info or encrypt; a decrypt flag in any spelling is refused.
fn age_ok(args: &[&str]) -> bool {
    let info = |a: &&str| flag(a).is_some_and(|f| matches!(f, "version" | "h" | "help"));
    #[rustfmt::skip]
    let enc = |a: &&str| flag(a).is_some_and(|f| matches!(f, "r" | "R" | "e" | "p" | "encrypt" | "recipient" | "recipients-file"));
    #[rustfmt::skip]
    let dec = |a: &&str| a.starts_with('-') && (flag(a) == Some("decrypt") || !a.starts_with("--") && a.contains('d'));
    args.is_empty() || args.iter().all(info) || (args.iter().any(enc) && !args.iter().any(dec))
}

#[rustfmt::skip]
fn mentions_secrets(text: &str) -> bool {
    ["sops", "SOPS_", "keys.txt", "hoist secrets"].iter().any(|k| text.contains(k))
        || text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .any(|t| matches!(t, "age" | "rage"))
}

/// Decrypt-lock (spec §4), an allowlist that fails closed: `sops`/`age` only print info or
/// encrypt, `hoist secrets edit|..`, the key directory, `$SOPS_AGE_KEY*`, `/proc/*/environ`,
/// env dumps naming sops, and shells reading stdin or code that mention secrets are refused.
/// Every word of a segment is tried as the command word (except in readers, file tools and
/// install-like packager commands); command strings in options are judged.
fn decrypt_rule(command: &str, depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return Some(too_deep());
    }
    let (text, bodies, targets) = strip_data_heredocs(command, depth);
    let (text, wrote) = (text.as_str(), mentions_secrets(&bodies));
    for inner in nested(text) {
        if let Some(why) = decrypt_rule(&inner, depth + 1) {
            return Some(why);
        }
    }
    let low = text.to_lowercase();
    #[rustfmt::skip]
    let key_dir = ["sops/age", "age/keys.txt"].iter().any(|k| text.contains(k)) || text.contains("keys.txt") && (low.contains("sops") || text.contains(".config") || text.contains("find"));
    if key_dir {
        return Some(decrypt_reason("the age key directory"));
    }
    if text.contains("$SOPS_AGE_KEY") || text.contains("${SOPS_AGE_KEY") {
        return Some(decrypt_reason("the age key"));
    }
    // Indirect expansion can name the variable at run time.
    if text.contains("${!")
        && (mentions_secrets(&low) || low.contains("ops_age") || text.contains("${!S"))
    {
        return Some(decrypt_reason("an indirect expansion"));
    }
    let (named, mentions) = (low.contains("sops"), mentions_secrets(text));
    let words = shell_words(text);
    let mut hits = if wrote { targets } else { Vec::new() };
    for segment in words.split(|w| w == SEP) {
        if mentions_secrets(&segment.join(" ")) {
            hits.extend(write_targets(segment));
        }
        let mut rest: Vec<&str> = segment.iter().map(String::as_str).collect();
        while rest.first().is_some_and(|w| assigned(w).is_some()) {
            rest.remove(0);
        }
        let Some(first) = rest.first() else { continue };
        let base = |w: &str| {
            w.rsplit(['/', '#'])
                .next()
                .unwrap_or(w)
                .split('@')
                .next()
                .unwrap_or(w)
                .to_string()
        };
        // Runners (`env`, `nice -n 5`, `timeout 5`, `sudo -u x`, `xargs`) run the command after
        // them: judge it as the command, in the directory `env -C` or `sudo -D` moves to. An
        // option they take that is not known may hide the real command, so after a runner no
        // command exempts its words from being tried as commands.
        let (cmd, cds, shell) = unwrap_runners(&rest);
        let script = shell.then(|| cmd.join(" ")).or_else(|| {
            let s = rest.iter().position(|w| base(w) == "strace")?;
            let p = s + rest[s..].iter().position(|w| STRACE_OPTIONAL.contains(w))?;
            let v = rest.get(p + 1)?;
            let glued = format!("{}={v}", rest[p]);
            Some(
                [&rest[..p], &[glued.as_str()], &rest[p + 2..]]
                    .concat()
                    .join(" "),
            )
        });
        if let Some(why) = script.and_then(|s| decrypt_rule(&s, depth + 1)) {
            return Some(why);
        }
        let wrapped = cmd.len() < rest.len();
        let name = base(cmd.first().unwrap_or(first));
        let args = &cmd[1.min(cmd.len())..];
        #[rustfmt::skip]
        let da = &args[..args.iter().position(|a| redirect(a)).unwrap_or(args.len())];
        let dump = match name.as_str() {
            "printenv" => da.is_empty() || named,
            "compgen" => da.iter().any(|a| matches!(*a, "-e" | "-v")),
            "env" => da
                .iter()
                .all(|a| a.starts_with('-') || assigned(a).is_some()),
            "set" => da.is_empty(),
            "export" | "declare" | "typeset" => {
                da.is_empty()
                    || da.iter().any(|a| matches!(*a, "-p" | "-x"))
                        && (named || da.iter().all(|a| a.starts_with('-')))
            }
            "ps" => da.iter().any(|a| {
                *a == "-E"
                    || !a.starts_with('-')
                        && a.contains('e')
                        && a.chars().all(|c| "aeuxwfl".contains(c))
            }),
            _ => false,
        };
        if dump {
            return Some(env_reason("the environment"));
        }
        // The verb skips git's global options and their values (`-C dir`, `-c k=v`).
        let (mut it, git) = (args.iter(), name == "git");
        let verb = loop {
            match it.next() {
                Some(a)
                    if git
                        && matches!(
                            *a,
                            "-C" | "-c"
                                | "--git-dir"
                                | "--work-tree"
                                | "--namespace"
                                | "--exec-path"
                                | "--config-env"
                        ) =>
                {
                    it.next();
                }
                Some(a) if a.starts_with('-') => {}
                other => break other.copied().unwrap_or(""),
            }
        };
        let long = |name: &str| {
            args.iter().enumerate().find_map(|(i, a)| {
                let v = a.strip_prefix(name).and_then(|r| r.strip_prefix('='));
                v.or_else(|| (*a == name).then(|| args.get(i + 1).copied()).flatten())
            })
        };
        // `bosun main-triage --run CMD` and `rg --pre CMD FILE` run a command.
        let run = match name.as_str() {
            "bosun" => long("--run").map(str::to_string),
            "rg" => long("--pre").map(|c| format!("{c} FILE")),
            #[rustfmt::skip]
            "git" if verb == "grep" => long("--open-files-in-pager").or_else(|| args.iter().enumerate().find_map(|(i, a)| a.strip_prefix("-O").map(|v| if v.is_empty() { args.get(i + 1).copied().unwrap_or("") } else { v }))).map(|c| format!("{c} FILE")),
            _ => None,
        };
        if let Some(why) = run.and_then(|c| decrypt_rule(&c, depth + 1)) {
            return Some(why);
        }
        // A pager or view runs on a file, and a shell there runs the search output as a
        // script: only known pagers pass, without expansions (`$SHELL`, `` `which sh` ``).
        let paged_at = rest.iter().position(|w| PAGED.contains(&base(w).as_str()));
        if paged_at.is_some_and(|p| !pagers_ok(&base(rest[p]), &words, &rest[p + 1..])) {
            return Some(decrypt_reason("an unknown or expanded pager"));
        }
        // `git grep --no-index` searches its paths (or the cwd) like `grep -r`, hidden ones too.
        // `grep.fallbackToNoIndex` (`-c`, `--config-env`, `GIT_CONFIG_KEY_<n>`) falls back to it.
        #[rustfmt::skip]
        let keys = words.iter().filter(|w| assigned(w).is_some_and(|k| k.starts_with("GIT_CONFIG_KEY_"))).filter_map(|w| w.split_once('=').map(|p| p.1));
        #[rustfmt::skip]
        let fallback = args.windows(2).filter(|w| matches!(w[0], "-c" | "--config-env")).map(|w| w[1])
            .chain(args.iter().filter_map(|a| a.strip_prefix("-c").or(a.strip_prefix("--config-env=")))).chain(keys)
            .any(|v| v.to_lowercase().starts_with("grep.fallbacktonoindex"));
        // `-C dir` and `--work-tree dir` move where it searches: join them with each root.
        let at = |b: &str, r: &str| match r.starts_with(['/', '~', '$']) {
            true => r.to_string(),
            false => format!("{b}/{r}"),
        };
        #[rustfmt::skip]
        let (mut dir, mut gi) = (cds.iter().fold(None::<String>, |d, v| Some(d.map_or(v.to_string(), |b| at(&b, v)))), args.iter());
        while let Some(a) = gi.next() {
            #[rustfmt::skip]
            let v = match *a {
                "-C" | "--work-tree" => gi.next().copied(),
                "-c" | "--git-dir" | "--namespace" | "--exec-path" | "--config-env" => { gi.next(); None }
                _ if a.starts_with('-') => a.strip_prefix("--work-tree=").or_else(|| a.strip_prefix("-C").filter(|v| !v.is_empty())),
                _ => break,
            };
            if let Some(v) = v {
                dir = Some(dir.map_or(v.to_string(), |b| at(&b, v)));
            }
        }
        // A key root as the base is outside any repo, so the fallback may apply there whatever
        // config set it (`GIT_CONFIG_PARAMETERS`, a gitconfig file); `../x` is not one here,
        // but `../../root`, `../../home/x` and `../../x` are.
        let key_base = dir.as_deref().map(lexical).is_some_and(|b| {
            let segs: Vec<&str> = b.split('/').collect();
            let top = segs
                .iter()
                .rposition(|s| *s == "..")
                .map(|i| segs.get(i + 1));
            #[rustfmt::skip]
            let home = top.is_none_or(|s| s.is_some_and(|s| matches!(*s, "root" | "home")));
            // Two `..` climb past a repo's parent: `../../user` from `~/repo/sub` is home.
            #[rustfmt::skip]
            let rel = ["$PWD/", "${PWD}/", "\"$PWD\"/", "/proc/self/cwd/"].iter().find_map(|p| b.strip_prefix(p)).unwrap_or(&b);
            let deep = rel.split('/').take_while(|s| *s == "..").count() >= 2;
            key_root(&b) == 2 || key_root(&b) == 1 && (home || deep)
        });
        let no_index = fallback || key_base || args.iter().any(|a| long_prefix(a, "--no-index", 6));
        if git && verb == "grep" && no_index {
            #[rustfmt::skip]
            const VALUE: &[&str] = &["-e", "-f", "-A", "-B", "-C", "-m", "--max-depth", "--threads"];
            // `GIT_ICASE_PATHSPECS`, `GIT_GLOB_PATHSPECS` and their global flags widen every
            // path (`.CONFIG`).
            #[rustfmt::skip]
            let wide = words.iter().any(|w| matches!(assigned(w).unwrap_or(w), "GIT_ICASE_PATHSPECS" | "GIT_GLOB_PATHSPECS" | "--icase-pathspecs" | "--glob-pathspecs"));
            if wide {
                return Some(decrypt_reason("a pathspec env var on a git grep search"));
            }
            // The first bare word is the pattern, unless `-e` or `-f` gave it, also in a short
            // cluster (`-eX`, `-ieX`, `-ifpat`).
            #[rustfmt::skip]
            let mut pat = args.iter().any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains(['e', 'f']));
            let (mut roots, mut it, mut paths) = (Vec::new(), it.clone(), false);
            while let Some(a) = it.next() {
                match *a {
                    "--" if !paths => paths = true,
                    _ if !paths && VALUE.contains(a) => drop(it.next()),
                    _ if !paths && a.starts_with('-') => {}
                    // Pathspec magic (`:(exclude)x`, `:!x`, `:/`) searches the whole base; as the
                    // pattern it is judged as a root too, and the next word is a root.
                    _ if a.trim_start_matches(['$', '\'', '"']).starts_with(':') => {
                        pat = true;
                        roots.push(".");
                    }
                    _ if !std::mem::replace(&mut pat, true) => {}
                    _ => roots.push(*a),
                }
            }
            let joined: Vec<String> = match &dir {
                Some(b) if roots.is_empty() => vec![lexical(b)],
                Some(b) => roots.iter().map(|r| lexical(&at(b, r))).collect(),
                None => Vec::new(),
            };
            roots.extend(joined.iter().map(String::as_str));
            if key_search(&roots, true, true) {
                return Some(decrypt_reason("a search of the age key directory"));
            }
        }
        if name == "bosun" && !wrapped {
            continue;
        }
        if READERS.contains(&name.as_str()) {
            let env = |w: &&str| procenv(w);
            #[rustfmt::skip]
            let pos: Vec<&str> = da.iter().copied().filter(|a| !a.starts_with('-')).collect();
            #[rustfmt::skip]
            let pat_flag = args.iter().any(|a| matches!(*a, "-e" | "-f") || a.starts_with("--file"));
            let finds = SEARCHERS.contains(&name.as_str());
            let search = finds && !pat_flag;
            // A search pattern naming the key marker is data, unless it searches home or `.config`.
            let home = |a: &&str| {
                a.starts_with(['~', '/']) || a.contains("$HOME") || a.contains(".config")
            };
            let pat_ok = search && !pos.is_empty() && !pos[1..].iter().any(home);
            if args
                .iter()
                .any(|a| keyline(a) && !(pat_ok && pos.first() == Some(a)))
            {
                return Some(decrypt_reason("an age secret key"));
            }
            // A recursive search whose root holds the age key reads it, whatever the pattern:
            // `.config` and its ancestors always; home (hidden dirs included) for the grep
            // family, and for rg and ag only with --hidden, -u or -. .
            let (rg, always) = (
                matches!(name.as_str(), "rg" | "ag"),
                matches!(name.as_str(), "rg" | "ag" | "ack" | "ack-grep" | "rgrep"),
            );
            // Long options by unique prefix (`--rec`, `--der`, `--dir=rec`, ag `--hi`, `--u`).
            let short = |a: &&str| a.starts_with('-') && !a.starts_with("--");
            // `-d` in a short cluster takes the rest of it, or the next word (`-nd recurse`).
            #[rustfmt::skip]
            let d = args.iter().enumerate().filter(|(_, a)| short(a)).filter_map(|(i, a)| Some((i, a[1..].split_once('d')?)))
                .map(|(i, (_, v))| if v.is_empty() { args.get(i + 1).copied().unwrap_or("") } else { v });
            #[rustfmt::skip]
            let dirs = long_values(args, "--directories", 4).into_iter().chain(d)
                .any(|v| v.len() >= 3 && "recurse".starts_with(v));
            let long =
                |a: &&str, opts: &[(&str, usize)]| opts.iter().any(|(f, m)| long_prefix(a, f, *m));
            #[rustfmt::skip]
            let recursive = always || dirs || args.iter().any(|a| long(a, &[("--recursive", 5), ("--dereference-recursive", 5)]) || short(a) && a.contains(['r', 'R']));
            #[rustfmt::skip]
            let hidden = !rg || args.iter().any(|a| long(a, &[("--hidden", 4), ("--unrestricted", 3)]) || short(a) && a.contains(['u', '.']));
            if finds && key_search(&pos, recursive, hidden) {
                return Some(decrypt_reason("a search of the age key directory"));
            }
            let cfg = |a: &&str| a.contains(".config/") && a.contains(['*', '?', '[', ']']);
            if matches!(name.as_str(), "cat" | "head" | "tail") && args.iter().any(cfg) {
                return Some(decrypt_reason("a glob in .config"));
            }
            if args.iter().any(env)
                && !(search && pos.first().is_some_and(env) && !pos[1..].iter().any(env))
            {
                return Some(env_reason("the process environment"));
            }
            // ack and ugrep take commands in options (`--pager`, `--view`, `--filter`, abbreviated)
            // and from the environment, so only their pattern is data.
            let paged = PAGED.contains(&name.as_str());
            if !paged && !wrapped {
                continue;
            }
            let filters = match paged && name.starts_with('u') {
                true => long_values(args, "--filter", 6),
                false => Vec::new(),
            };
            for c in filters.iter().flat_map(|f| f.split(',')) {
                let c = c.split_once(':').map_or(c, |p| p.1);
                if let Some(why) = decrypt_rule(c, depth + 1) {
                    return Some(why);
                }
            }
        }
        #[rustfmt::skip]
        let sub = args.iter().find(|a| !a.starts_with('-')).is_some_and(|a| PKG_SUBS.contains(a));
        let safe = !wrapped
            && (FILE_TOOLS.contains(&name.as_str()) || PACKAGERS.contains(&name.as_str()) && sub);
        // Data words: message and query option values, search patterns.
        let (msg, vi) = (
            matches!(name.as_str(), "git" | "gh"),
            segment.iter().position(|x| x == verb),
        );
        let (gs, ss) = (
            name == "git" && verb == "grep",
            name == "gh" && verb == "search",
        );
        let api = !wrapped && (name.as_str(), verb) == ("gh", "api");
        let mut data = vec![false; segment.len()];
        let (mut skip, mut pat_done) = (false, false);
        for (i, w) in segment.iter().enumerate() {
            let opt = w.trim_start_matches('-').split('=').next().unwrap_or("");
            let opts: &[&str] = match (name.as_str(), verb) {
                ("gh", _) => GH_DATA,
                ("git", "log") => &["S", "G", "grep"],
                ("git", "grep") => &["e"],
                _ => &["m", "message"],
            };
            let pat = !w.starts_with('-') && vi.is_some_and(|v| i > v) && (ss || gs && !pat_done);
            pat_done |= pat;
            if std::mem::take(&mut skip) || pat {
                data[i] = true;
            } else if msg && w.starts_with('-') && opts.contains(&opt) {
                (data[i], skip) = (true, !w.contains('='));
            }
        }
        // ack and ugrep: the `-e`/`--regexp` values, else the first bare word that is not the
        // value of a long option.
        let paged = PAGED.contains(&name.as_str());
        if paged {
            let (a0, mut seen) = (segment.len() - args.len(), false);
            let e = da
                .iter()
                .any(|a| matches!(*a, "-e" | "--regexp") || a.starts_with("--regexp="));
            for (j, a) in da.iter().enumerate() {
                let prev = if j == 0 { "" } else { da[j - 1] };
                // ugrep's `--pager` and `--view` take no next word.
                #[rustfmt::skip]
                let bare = name.starts_with('u') && (long_prefix(prev, "--pager", 5) || long_prefix(prev, "--view", 4));
                let value = prev.starts_with("--") && !prev.contains('=') && !bare;
                #[rustfmt::skip]
                let pat = if e { matches!(prev, "-e" | "--regexp") || a.starts_with("--regexp=") }
                    else { !seen && !a.starts_with('-') && !value };
                seen |= pat;
                data[a0 + j] |= pat;
            }
        }
        // After a runner the command may be misread: no word is data.
        if wrapped {
            data.fill(false);
        }
        // Options run command strings: a word or option value with whitespace is a command.
        for w in segment
            .iter()
            .zip(&data)
            .filter(|(_, d)| !**d)
            .map(|(w, _)| w)
        {
            let w = w.as_str();
            if procenv(w) {
                return Some(env_reason("the process environment"));
            }
            let t = if assigned(w).is_some() {
                w.split_once('=').map_or(w, |p| p.1)
            } else {
                w
            };
            if keyline(w) {
                return Some(decrypt_reason("an age secret key"));
            }
            if !safe && !(api && !w.starts_with('-')) && t.contains("SOPS_AGE_KEY") {
                return Some(decrypt_reason("the age key"));
            }
            if w.contains(".config/") && w.contains(['*', '?', '[', ']']) {
                return Some(decrypt_reason("a glob in .config"));
            }
            let value = w.split_once('=').map(|(_, v)| v);
            let strs = [
                Some(w.strip_prefix("-O").unwrap_or(w)).filter(|w| w.contains(char::is_whitespace)),
                value.filter(|v| v.contains(char::is_whitespace) || v.starts_with(['|', '!'])),
            ];
            for c in strs.into_iter().flatten() {
                if let Some(why) = decrypt_rule(c.trim_start_matches(['|', '!']), depth + 1) {
                    return Some(why);
                }
            }
        }
        let off = segment.len() - rest.len();
        let tried = if safe || paged && !wrapped {
            0
        } else {
            rest.len()
        };
        for i in 0..tried {
            if data[i + off] {
                continue;
            }
            let (n, after) = (base(rest[i]), &rest[i + 1..]);
            let tgt = |a: &str| {
                hits.iter()
                    .any(|t| t.trim_start_matches("./") == a.trim_start_matches("./"))
            };
            #[rustfmt::skip]
            let refused = match n.as_str() {
                "sops" => !sops_ok(&after[..after.iter().position(|a| redirect(a)).unwrap_or(after.len())]),
                "age" | "rage" => !age_ok(&after[..after.iter().position(|a| redirect(a)).unwrap_or(after.len())]),
                "hoist" => after.iter().position(|a| *a == "secrets").is_some_and(|p| after[p + 1..].iter().take_while(|a| **a != "--" && **a != "exec").any(|a| HOIST_SECRETS.contains(a))),
                "alias" => after.iter().filter_map(|a| a.split_once('=')).any(|(_, v)| matches!(base(v.split(' ').next().unwrap_or("")).as_str(), "sops" | "age" | "rage")),
                "awk" | "gawk" | "mawk" => after.iter().any(|a| a.contains("ENVIRON")),
                "jq" => after.iter().any(|a| a.contains("$ENV") || a.split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.'))).any(|t| t == "env" || t.starts_with("env."))),
                n if INTERPS.contains(&n) && after.iter().any(|a| ENV_READS.iter().any(|e| a.contains(e) && (*e != "environ" || runs_code(n, after)))) => true,
                _ if i == 0 && tgt(rest[0]) => true,
                n if (SHELLS.contains(&n) || n == ".") && after.iter().any(|a| tgt(a)) => true,
                n if SHELLS.contains(&n) && mentions => {
                    let interp = INTERPS.contains(&n);
                    let code = runs_code(n, after);
                    let file = after.iter().any(|a| !a.starts_with(['-', '<']));
                    let stdin = after.iter().any(|a| matches!(*a, "-s" | "-") || a.starts_with('<'));
                    if code { interp || after.iter().any(|a| a.contains("{}") || a.contains('$')) } else { !file || stdin }
                }
                _ => false,
            };
            if refused {
                // Code or a filter that reads the environment (and names no secrets tool).
                let env_read = !mentions
                    && (matches!(n.as_str(), "awk" | "gawk" | "mawk" | "jq")
                        || INTERPS.contains(&n.as_str())
                            && after
                                .iter()
                                .any(|a| ENV_READS.iter().any(|e| a.contains(e))));
                if env_read {
                    return Some(env_reason(&format!("the environment (`{n}`)")));
                }
                return Some(decrypt_reason(&format!("`{n}`")));
            }
        }
    }
    None
}

fn heredoc_at(line: &str) -> Option<usize> {
    let (mut quote, b) = (None, line.as_bytes());
    for (i, c) in line.char_indices() {
        let op = c == '<' && b.get(i + 1) == Some(&b'<');
        // In double quotes, `<<` counts after an unclosed `$(`: `"$(cat <<'EOF'`.
        let sub = || line[..i].matches("$(").count() > line[..i].matches(')').count();
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), _) if op && sub() => return (b.get(i + 2) != Some(&b'<')).then_some(i),
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, _) if op => return (b.get(i + 2) != Some(&b'<')).then_some(i),
            _ => {}
        }
    }
    None
}

/// The text without the bodies of quoted-delimiter heredocs (unless their line has a shell),
/// those bodies, and the files the opening lines write (`> f`, `>> f`, `tee f`). When only
/// interpreters (no shell) are on the line, an interpreter owns the heredoc and the line writes
/// no file, the body stays code with its prose literals emptied.
fn strip_data_heredocs(command: &str, depth: usize) -> (String, String, Vec<String>) {
    let (mut out, mut bodies) = (String::new(), String::new());
    let (mut skip, mut targets) = (None::<(String, bool, bool)>, Vec::new());
    for line in command.split_inclusive('\n') {
        if let Some((delim, dash, code)) = &skip {
            let l = line.trim_end_matches('\n');
            let l = if *dash { l.trim_start_matches('\t') } else { l };
            if l != delim && l.starts_with(delim.as_str()) {
                return (command.to_string(), bodies, targets);
            }
            if l == delim {
                skip = None;
                out.push_str(line);
            } else if *code {
                out.push_str(&literals::blank_prose(line, depth));
            } else {
                bodies.push_str(line);
            }
            continue;
        }
        out.push_str(line);
        if let Some(at) = heredoc_at(line) {
            let spec = line[at + 2..].trim_start_matches('-').trim_start();
            let dash = line[at + 2..].starts_with('-');
            let quote = spec.chars().next().filter(|c| matches!(c, '\'' | '"'));
            let words = shell_words(line);
            let names = || words.iter().map(|w| w.rsplit('/').next().unwrap_or(w));
            let reads = names().any(|w| SHELLS.contains(&w));
            // Only interpreters (`python3 -`), no shell or `source`/`xargs`/`eval`; the command
            // of the segment that holds `<<` is one (not `cat <<'EOF' | python3 ..`), and the
            // line writes no file (`python3 - > x.sh <<'EOF'` is a script for later).
            let seg = &line[line[..at].rfind(['|', ';', '&', '(']).map_or(0, |i| i + 1)..at];
            let owner = shell_words(seg).into_iter().find(|w| assigned(w).is_none());
            let owned =
                owner.is_some_and(|w| INTERPS.contains(&w.rsplit('/').next().unwrap_or(&w)));
            let writes = write_targets(&words);
            let interp = owned
                && writes.is_empty()
                && names().all(|w| !SHELLS.contains(&w) || INTERPS.contains(&w));
            if let Some(q) = quote
                && let Some(end) = spec[1..].find(q)
                && (!reads || interp)
            {
                skip = Some((spec[1..=end].to_string(), dash, reads));
                targets.extend(writes);
            }
        }
    }
    (out, bodies, targets)
}

fn http_rule(segment: &str) -> Option<&'static str> {
    if GRAPHQL_WRITES.iter().any(|m| segment.contains(m)) {
        return Some("GraphQL merge or ref mutation");
    }
    let github = segment.contains("api.github.com") || segment.contains("/repos/");
    (github && HTTP_WRITES.iter().any(|w| segment.contains(w)))
        .then_some("GitHub API merge, ref, release or auto-merge write")
}

fn gh_rule(args: &[&str], segment: &str) -> Option<&'static str> {
    // Quotes and backslashes do not hide a word (`gh pr "merge"`, `gh p""r merge`), and a global
    // `-R o/r` or `--repo o/r` before the command does not move it.
    let clean: Vec<String> = args
        .iter()
        .map(|a| a.replace(['\'', '"', '\\'], ""))
        .collect();
    let mut i = 0;
    while i < clean.len() && clean[i].starts_with('-') {
        let a = clean[i].as_str();
        let value_next = matches!(a, "-R" | "--repo");
        i += if value_next { 2 } else { 1 };
    }
    let at = i.min(clean.len());
    // `gh api` is judged on the words as written (quotes shape a GraphQL query).
    if clean.get(at).map(String::as_str) == Some("api") {
        return api_rule(&args[at + 1..], segment);
    }
    let mut args: Vec<&str> = clean[at..].iter().map(String::as_str).collect();
    // `gh pr -R o/r merge 5`, `gh release --repo=o/r create v1`: a flag after the group word does
    // not move the command.
    if !args.is_empty() {
        let mut j = 1;
        while j < args.len() && args[j].starts_with('-') {
            j += if matches!(args[j], "-R" | "--repo") {
                2
            } else {
                1
            };
        }
        args.drain(1..j.min(args.len()));
    }
    match &args[..] {
        ["pr", "merge", ..] => Some("gh pr merge"),
        [
            "release",
            "create" | "edit" | "upload" | "delete" | "delete-asset",
            ..,
        ] => Some("a release write makes or moves a tag"),
        ["alias", "set" | "import", ..] => Some("a gh alias makes commands the guard cannot see"),
        ["workflow", "run", ..] => Some("a workflow run can release or deploy"),
        ["secret" | "variable", "set" | "delete" | "remove", ..] => {
            Some("secrets and variables are the owner's")
        }
        _ => None,
    }
}

/// `gh api ARGS`: the args after `api`.
fn api_rule(rest: &[&str], segment: &str) -> Option<&'static str> {
    // Quotes and backslashes do not hide a path word (`pulls/1/'merge'`).
    let clean: Vec<String> = rest
        .iter()
        .map(|a| a.replace(['\'', '"', '\\'], ""))
        .collect();
    if GRAPHQL_WRITES.iter().any(|m| segment.contains(m)) {
        Some("GraphQL merge or ref mutation")
    } else if graphql_hidden(rest) {
        Some("a GraphQL query the guard cannot see")
    } else if gh_api_read(rest, segment) {
        None
    } else if clean.iter().any(|a| a.contains("/merge")) {
        Some("merge API")
    } else if clean.iter().any(|a| a.contains("/releases")) {
        Some("a release makes a tag")
    } else if clean.iter().any(|a| a.contains("auto_merge")) {
        Some("auto-merge API")
    } else if clean
        .iter()
        .any(|a| a.contains("git/refs") || a.contains("/tags"))
    {
        Some("ref or tag API")
    } else {
        None
    }
}

/// A `gh api` read: every method flag says GET and there is no field or input flag (`-f`, `-F`,
/// `--field`, `--raw-field`, `--input`, which make gh default to POST). Fails closed: a segment
/// with `$`, a backtick, a backslash or unbalanced quotes (cut at a quoted separator) is not a
/// read, nor is a short-flag cluster holding `X`, `f` or `F` other than `-X GET`.
fn gh_api_read(args: &[&str], segment: &str) -> bool {
    let odd = |q: char| segment.matches(q).count() % 2 == 1;
    if segment.contains(['$', '`', '\\']) || odd('\'') || odd('"') {
        return false;
    }
    let args: Vec<String> = args.iter().map(|a| a.replace(['\'', '"'], "")).collect();
    let get = |m: &str| m.eq_ignore_ascii_case("GET");
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if matches!(a, "-X" | "--method") {
            if !args.get(i + 1).is_some_and(|m| get(m)) {
                return false;
            }
            i += 1;
        } else if let Some(m) = a.strip_prefix("--method=").or_else(|| a.strip_prefix("-X")) {
            if !get(m) {
                return false;
            }
        } else if ["--field", "--raw-field", "--input"]
            .iter()
            .any(|f| a.starts_with(f))
            || (a.starts_with('-') && !a.starts_with("--") && a.contains(['X', 'f', 'F']))
        {
            return false;
        }
        i += 1;
    }
    true
}

/// A `gh api graphql` call whose query the guard cannot read: `--input`, a `query=@file`, or a
/// query outside single quotes with `$` or a backtick.
fn graphql_hidden(args: &[&str]) -> bool {
    let unq = |a: &str| a.trim_matches(['\'', '"']).to_string();
    if !args.iter().any(|a| {
        let a = unq(a);
        a == "graphql" || a.ends_with("/graphql")
    }) {
        return false;
    }
    let mut i = 0;
    while let Some(&a) = args.get(i) {
        i += 1;
        if a.starts_with("--input") {
            return true;
        }
        let value = match a {
            "-f" | "-F" | "--field" | "--raw-field" => args.get(i).copied(),
            _ if a.starts_with("--") => a
                .strip_prefix("--field=")
                .or_else(|| a.strip_prefix("--raw-field=")),
            _ => a.strip_prefix("-f").or_else(|| a.strip_prefix("-F")),
        };
        let Some(q) = value.and_then(|v| v.trim_start_matches(['\'', '"']).strip_prefix("query="))
        else {
            continue;
        };
        if q.starts_with('@') || (!q.starts_with('\'') && q.contains(['$', '`', '\\'])) {
            return true;
        }
    }
    false
}

/// A git config key that defines an alias (`alias.x`, any case, quoted or not).
fn alias_key(a: &str) -> bool {
    a.trim_start_matches(['\'', '"'])
        .to_ascii_lowercase()
        .starts_with("alias.")
}

const GIT_ALIAS: &str = "a git alias hides the command it runs";

const PUSH_CONFIG: &str = "git config sets where this push goes (`remote.<r>.push`, `remote.<r>.mirror` or `push.default`); name the refspec, as in `git push -u origin claude/fix-1`";

const PUSH_ALONE: &str =
    "a push without `src:dst` goes where git config says, which another command here may change";

/// `PUSH_ALONE` with the way out for this push: its own branch as `b:b`.
fn alone_fix(words: &[&str]) -> String {
    let after = words.iter().skip_while(|w| **w != "push").skip(1);
    let plain: Vec<&str> = after.copied().filter(|w| !w.starts_with('-')).collect();
    let b = plain
        .get(1)
        .map_or("<branch>", |b| b.trim_matches(['\'', '"']));
    let b = b.split(':').next().unwrap_or(b);
    format!("{PUSH_ALONE}; run the push as its own command, or name both sides: `{b}:{b}`")
}

const FETCH_PLAIN: &str = "a fetch or pull next to other shell commands, whose environment the guard cannot read, or a git command that runs a local command (`ext::`, `fd::`, `-c`, `--upload-pack`, `--exec-path` with a fetch); run a plain git fetch/pull as its own command (`cd` and other `git` commands may join it with `&&`)";

const GLOB_WORD: &str = "a glob (`*`, `?`, `[`) in a command name or a git word, which bash expands against files the guard cannot see; spell the command name and git words literally, or quote them";

const FETCH_MOVES: &str = "a fetch into the default branch or into a ref the guard cannot read; name the branch literally";

const BUILT_SUB: &str = "a git subcommand built by the shell, which the guard cannot see: \
    write the subcommand out, not a variable or substitution";

const UNREAD_PUSH: &str = "a push target the guard cannot read; name the branch literally, as in `git push -u origin claude/fix-1`";

/// A command that is one plain `git` call: no other command, `$`, backtick, group, redirect
/// (but `2>&1`), assignment, `source` or `eval`; a last `| tail -N` or `| head -N` only reads.
fn alone(command: &str) -> bool {
    let c = command.trim().replace("2>&1", "");
    let c = match c.rsplit_once('|') {
        Some((git, tail)) => {
            let w: Vec<&str> = tail.split_whitespace().collect();
            let n = |x: &str| {
                x.trim_start_matches('-')
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            };
            let reads = matches!(w.as_slice(), [t, x] if matches!(*t, "tail" | "head") && n(x))
                || matches!(w.as_slice(), [t, "-n", x] if matches!(*t, "tail" | "head") && n(x));
            if !reads {
                return false;
            }
            git.trim_end().to_string()
        }
        None => c,
    };
    c.starts_with("git ") && !c.contains(['\n', ';', '&', '|', '$', '`', '(', ')', '<', '>'])
}

/// A git config key that sets where a push goes: `remote.<r>.push`, `remote.<r>.mirror`.
/// A key with a backslash is one the guard cannot read: fail closed.
fn push_key(k: &str) -> bool {
    let k = cfg_key(k);
    k.contains('\\')
        || k == "include.path"
        || k == "push.followtags"
        || k.starts_with("includeif.")
        || k.starts_with("remote.") && (k.ends_with(".push") || k.ends_with(".mirror"))
}

/// A config key as git reads it: quotes removed, lowercase.
fn cfg_key(k: &str) -> String {
    k.replace(['\'', '"'], "").to_ascii_lowercase()
}

/// A ref as git reads it: `//` and `/./` collapse and a leading `./` goes (`refs//tags/v1`).
fn norm_ref(r: &str) -> String {
    let mut r = r.trim_start_matches("./").to_string();
    while r.contains("//") || r.contains("/./") {
        r = r.replace("//", "/").replace("/./", "/");
    }
    r
}

/// Whether a push's destination names the default branch (`main`, `refs/heads/main`, and
/// `heads/main`, which git expands to `refs/heads/main`).
fn names_default(dst: &str, default_branch: &str) -> bool {
    let d = norm_ref(dst);
    let d = d.strip_prefix("refs/").unwrap_or(&d);
    d == default_branch || d.strip_prefix("heads/") == Some(default_branch)
}

/// `push.default` values that push only the current branch to its own name (`HEAD` is judged).
fn head_only(v: &str) -> bool {
    matches!(
        v.trim_matches(['\'', '"']).to_ascii_lowercase().as_str(),
        "simple" | "current" | "nothing"
    )
}

/// `git subtree push -P <prefix> <repository> [<commit>:]<ref>`: refused when the ref is
/// missing or any word names the default branch or a tag (quoted messages split into words,
/// so this fails closed).
fn subtree_push(rest: &[&str], default_branch: &str) -> Option<&'static str> {
    if rest.iter().any(|a| a.contains(['$', '`', '\\'])) {
        return Some(UNREAD_PUSH);
    }
    let (mut words, mut it) = (Vec::new(), rest.iter().map(|a| a.trim_matches(['\'', '"'])));
    while let Some(a) = it.next() {
        if matches!(
            a,
            "-P" | "--prefix" | "-m" | "--message" | "-b" | "--branch" | "--onto" | "--annotate"
        ) {
            it.next();
        } else if !a.starts_with('-') {
            words.push(a);
        }
    }
    if words.len() < 2 {
        return Some(UNREAD_PUSH);
    }
    for w in words {
        let dst = w.rsplit(':').next().unwrap_or(w);
        if w.starts_with('+') || dst.contains("refs/tags/") || version_like(dst) {
            return Some("force, tag or delete push");
        }
        if names_default(dst, default_branch) {
            return Some("push to the default branch");
        }
    }
    None
}

/// `git merge --ff-only [origin/<default>|@{u}|@{upstream}]`: catches the local branch up
/// with its remote, never brings in other work.
fn ff_only(rest: &[&str], default_branch: &str) -> bool {
    let refs: Vec<&&str> = rest.iter().filter(|a| **a != "--ff-only").collect();
    rest.len() == refs.len() + 1
        && refs.len() <= 1
        && refs.iter().all(|r| {
            matches!(**r, "@{u}" | "@{upstream}")
                || r.strip_prefix("origin/") == Some(default_branch)
        })
}

/// The value options of `git fetch`; a long one also by a prefix (`--dept 1`), as git takes it.
const FETCH_VALUES: &str = "-s --strategy -X --strategy-option --depth --deepen --shallow-since \
    --shallow-exclude -j --jobs --upload-pack --negotiation-tip --filter -o --server-option \
    --recurse-submodules-default --submodule-prefix --refmap";

/// The flag letters of a short-option cluster (`-pPt`) up to its first value option (`o`, `j`,
/// `s`, `X`), whose value is the rest of the cluster or, when it is the last letter, the next word.
fn short_flags(a: &str) -> Option<&str> {
    let c = a
        .strip_prefix('-')
        .filter(|c| !c.is_empty() && !c.starts_with('-'))?;
    Some(match c.find(['o', 'j', 's', 'X']) {
        Some(i) => &c[..=i],
        None => c,
    })
}

/// The plain words of `git fetch` (the remote, then refspecs) and its `--refmap` values.
fn fetch_words(rest: &[&str]) -> (Vec<String>, Vec<String>) {
    let takes_value = |a: &str| {
        FETCH_VALUES.split_whitespace().any(|v| v == a)
            // `-po origin`: a cluster whose first value option is its last letter takes the next
            // word (`-Xtheirs` holds its value).
            || short_flags(a).is_some_and(|f| f.len() + 1 == a.len() && f.ends_with(['o', 'j', 's', 'X']))
            || (a.len() > 3
                && a.starts_with("--")
                && a != "--recurse-submodules"
                && FETCH_VALUES.split_whitespace().any(|v| v.starts_with(a)))
    };
    let (mut words, mut maps) = (Vec::new(), Vec::new());
    let mut it = rest.iter().map(|a| a.replace(['\'', '"'], ""));
    while let Some(a) = it.next() {
        // A `--refmap` value (also `--refm`, `--refma`) is a refspec too.
        let key = a.split('=').next().unwrap_or(&a);
        if key.len() >= 6 && "--refmap".starts_with(key) {
            match a.split_once('=') {
                Some((_, m)) => maps.push(m.to_string()),
                None => maps.extend(it.next()),
            }
        } else if takes_value(&a) {
            it.next();
        } else if !a.starts_with('-') {
            words.push(a);
        }
    }
    (words, maps)
}

/// A local-repository fetch word: `file:`, a variable, `./`, `../`, `/`, `~` (`./ev:il`), `.x`.
fn local_word(w: &str) -> bool {
    w.starts_with("file:")
        || ["./", "../", "/", "~"].iter().any(|p| w.starts_with(p))
        || w.contains(['$', '`'])
        || (!w.contains(':') && w.starts_with('.'))
}

/// Whether the fetch reads only `origin` or `upstream` (or the default remote; not `--all` or
/// `--multiple` in any prefix): any other name may be a directory (`evil2`).
fn fetch_trusted(rest: &[&str]) -> bool {
    let (words, _) = fetch_words(rest);
    let many = |a: &&str| a.len() >= 4 && ["--all", "--multiple"].iter().any(|o| o.starts_with(a));
    !rest.iter().any(many)
        && !words.iter().any(|w| local_word(w))
        && words
            .first()
            .is_none_or(|w| matches!(w.as_str(), "origin" | "upstream"))
}

/// Why a command is refused when a git fetch or pull is not a plain chain of `git ..`/`cd ..`
/// segments, or a git command runs a local command (spec §4, fetch rules).
fn fetch_not_plain(command: &str) -> Option<&'static str> {
    let text = split_heredocs(command).0.replace("2>&1", "");
    let cut = |c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '{' | '}');
    // `fe""tch` and `\\fetch` run fetch too.
    let base = |w: &str| {
        w.replace(['\'', '"', '\\'], "")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string()
    };
    let bare = |w: &str| matches!(base(w).trim_start_matches("git-"), "fetch" | "pull");
    let git = |w: &str| base(w) == "git" || base(w).starts_with("git-");
    // `$'\\x65xt::'` and `$"..."` quoting can spell any word: refused in any git command.
    if !text.split(cut).any(git) {
        return None;
    }
    if text.contains("$'") || text.contains("$\"") {
        return Some(FETCH_PLAIN);
    }
    // `ext::<cmd>` and `fd::` transports run a command or read a descriptor, in any git command
    // (`remote add x 'ext::sh p'`, then a fetch), however quoted.
    let unquoted = text.replace(['\'', '"', '\\'], "");
    if unquoted.contains("ext::") || unquoted.contains("fd::") {
        return Some(FETCH_PLAIN);
    }
    // `git remote [-v] update` fetches too.
    let toks: Vec<&str> = unquoted.split(cut).filter(|t| !t.is_empty()).collect();
    let remote = toks.iter().position(|t| *t == "remote");
    let update = remote.is_some_and(|p| toks[p..].contains(&"update"));
    if !text.split(cut).any(bare) && !update {
        return None;
    }
    // Any `$` in a fetch or pull (a lone one too) hides a word the guard cannot read.
    if text.contains('$') {
        return Some(FETCH_MOVES);
    }
    let plain = |seg: &str| {
        let w: Vec<&str> = seg.split_whitespace().collect();
        match w.first() {
            // Config (`-c protocol.ext.allow=always`), `--upload-pack` (`-u`, `--upl`) and
            // `--exec-path` run code: words are read unquoted (`'-c'`).
            Some(c) if *c == "git" || base(c).starts_with("git-") => {
                let u: Vec<String> = w.iter().map(|a| a.replace(['\'', '"', '\\'], "")).collect();
                // A git run inside another subcommand (`bisect run`, `submodule foreach`) with a
                // fetch: the guard cannot tell where it reads config.
                let mut sub = 1;
                while u.get(sub).is_some_and(|a| a.starts_with('-')) {
                    let pair = [
                        "-C",
                        "-c",
                        "--git-dir",
                        "--work-tree",
                        "--namespace",
                        "--config-env",
                    ];
                    sub += 1 + usize::from(pair.contains(&u[sub].as_str()));
                }
                let inner = u.iter().skip(sub + 1).position(|a| git(a));
                let fetch = |a: &String| bare(a) || a == "update";
                if inner.is_some_and(|p| u[sub + 1 + p..].iter().any(fetch)) {
                    return false;
                }
                !(u.iter().any(|a| bare(a))
                    && u.iter().any(|a| {
                        a == "-c"
                            || ["--config-env", "--upl", "--ex"]
                                .iter()
                                .any(|p| a.starts_with(p))
                            || short_flags(a).is_some_and(|f| f.contains('u'))
                    }))
            }
            Some(&"cd") => w.len() <= 2,
            Some(&("tail" | "head")) => {
                w.len() == 2 && w[1].trim_start_matches('-').parse::<u32>().is_ok()
            }
            _ => false,
        }
    };
    // A lone `git` segment is judged by `git_rule`, which refuses `\\` in a fetch.
    let segs: Vec<&str> = text
        .split([';', '&', '|', '\n'])
        .filter(|g| !g.trim().is_empty())
        .collect();
    (text.contains(['`', '(', ')', '{', '}', '<', '>'])
        || (segs.len() > 1 && text.contains('\\'))
        || segs.iter().any(|seg| !plain(seg)))
    .then_some(FETCH_PLAIN)
}

/// An unquoted `*`, `?` or `[` in a command word or any git word (`/usr/bin/gi[t]`, `git f?tch`);
/// `[` and `[[` alone are the test builtin.
fn glob_rule(command: &str, top: bool) -> Option<&'static str> {
    let (mut segs, mut word, mut glob, mut quote, mut esc) =
        (vec![Vec::new()], String::new(), false, None, false);
    // Words with whether they hold an unquoted glob and whether any part was quoted.
    let mut quoted = false;
    for c in split_heredocs(command).0.chars().chain([' ']) {
        match (quote, c) {
            _ if esc => (esc, _) = (false, word.push(c)),
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\\') => esc = true,
            (None, '\'' | '"') => (quote, quoted) = (Some(c), true),
            (None, c) if c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')') => {
                if !word.is_empty() || glob || quoted {
                    let w = (std::mem::take(&mut word), std::mem::take(&mut glob));
                    segs.last_mut()?
                        .push((w.0, w.1, std::mem::take(&mut quoted)));
                }
                if !c.is_whitespace() || c == '\n' {
                    segs.push(Vec::new());
                }
            }
            (None, c) => (glob, _) = (glob || matches!(c, '*' | '?' | '['), word.push(c)),
        }
    }
    static VAR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\$(?:\{([A-Za-z_]\w*)\}|([A-Za-z_]\w*))$").expect("static regex")
    });
    // Variables the shell sets itself (they persist across calls, like `PWD` after a `cd`).
    const SHELL_VARS: &str = "_ PWD OLDPWD HOME IFS PS1 PS2 PS3 PS4 PROMPT_COMMAND ENV SHELLOPTS \
        BASHOPTS CDPATH PATH SHELL REPLY OPTARG OPTIND RANDOM LINENO SECONDS FUNCNAME GROUPS \
        DIRSTACK PIPESTATUS HOSTNAME HOSTTYPE MACHTYPE OSTYPE PPID UID EUID SHLVL COLUMNS LINES";
    static BRACE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\{[^{}]*(,|\.\.)[^{}]*\}").expect("static regex"));
    // A glob whose last component can be `git` (`gi[t]`, `g?t`, `*`; `[..]` is one character).
    static CLASS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[[^\]]*\]").expect("static regex"));
    let names = |w: &str, name: &str| {
        let pat = CLASS.replace_all(w.rsplit('/').next().unwrap_or(w), "?");
        let re = regex::escape(&pat).replace(r"\*", ".*").replace(r"\?", ".");
        Regex::new(&format!("^{re}$")).is_ok_and(|r| r.is_match(name))
    };
    let names_git = |w: &str| names(w, "git");
    segs.iter().find_map(|seg| {
        let git = seg
            .iter()
            .any(|w| w.0.rsplit('/').next() == Some("git") || w.1 && names_git(&w.0));
        // A fetch, pull or update word in a quoted or `=` value of a git command runs a fetch
        // the guard cannot place (`rebase -x 'cd x && git fetch'`, `-c core.editor=..`); a
        // message or search value (`-m`, `--grep`, `--format`) is prose.
        let prose = [
            "-m",
            "--message",
            "--grep",
            "-S",
            "-G",
            "--format",
            "--pretty",
        ];
        let is_prose = |i: usize, w: &str| {
            i > 0 && prose.contains(&seg[i - 1].0.as_str())
                || prose
                    .iter()
                    .any(|f| w.starts_with(f) && (f.len() == 2 || w[f.len()..].starts_with('=')))
        };
        let gitw = |w: &str| {
            w.rsplit('/')
                .next()
                .is_some_and(|b| b == "git" || b.starts_with("git-"))
        };
        // A plain `"$NAME"` passes only as a global `-C`, `--git-dir` or `--work-tree` value (git
        // takes those literally; a positional one can expand to an option), never after a
        // builtin or function that can set a variable (`source`, `eval`, `read`, `f() {..}`).
        let g = seg.iter().position(|w| !w.2 && gitw(&w.0)).unwrap_or(0);
        let mut sub = g + 1;
        while seg.get(sub).is_some_and(|w| w.0.starts_with('-')) {
            let pair = ["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--config-env"];
            sub += 1 + usize::from(pair.contains(&seg[sub].0.as_str()));
        }
        let sets = command
            .split(|c: char| c.is_whitespace() || ";&|()".contains(c))
            .any(|t| {
                "source . eval declare typeset read export printf mapfile readarray local let function"
                    .split(' ')
                    .any(|b| b == t)
            })
            || command.contains("()");
        let push = seg.get(sub).is_some_and(|s| s.0 == "push");
        let place = |i: usize| {
            let prev = i.checked_sub(1).map_or("", |p| seg[p].0.as_str());
            !sets && i < sub && matches!(prev, "-C" | "--git-dir" | "--work-tree")
        };
        let names_fetch = |w: &str, i: usize| {
            let cut = |c: char| c.is_whitespace() || matches!(c, ';' | '&' | '|' | '(' | ')' | '=');
            // Any `$` or backtick builds words (`${IFS}`, `$(..)`), unless the value is one
            // variable (`"$BRANCH"`) the command does not set: it comes from the session.
            let var = VAR.captures(w).and_then(|c| c.get(1).or(c.get(2)));
            // `git push` refuses any `$` itself ("name the branch literally").
            let session = push || place(i)
                && var.is_some_and(|n| {
                    let n = n.as_str();
                    if n.starts_with("BASH") || SHELL_VARS.split_whitespace().any(|v| v == n) {
                        return false;
                    }
                    let rest = command
                        .replace(&format!("${{{n}}}"), "")
                        .replace(&format!("${n}"), "");
                    !rest.contains(n)
                });
            // Inner quotes and escapes go (`git 'fetch'`, `f"e"tch`); a glob may name one, and a
            // git-core program (`git-fetch`) runs git. A refspec (`a:b`) is no command word.
            let w = w.to_ascii_lowercase().replace(['\'', '"', '\\'], "");
            // Shell code has a space or an operator; a word that is itself a brace expansion
            // (`{a,b c}`) is judged by the brace rule.
            let shell = |c: char| c.is_whitespace() || ";&|(){}<>".contains(c);
            let code = w.contains(shell) && !w.starts_with('{');
            let built = w.contains(['$', '`']) && !session || code && BRACE.is_match(&w);
            built
                || w.split(cut).any(|t| {
                    let base = t.rsplit('/').next().unwrap_or(t);
                    let glob = t.contains(['*', '?', '[']) && !t.contains(':');
                    base.starts_with("git-")
                        || ["fetch", "pull", "update", "git-fetch", "git-pull"]
                            .iter()
                            .any(|n| base == *n || glob && names(t, n))
                })
        };
        if seg.iter().any(|w| !w.2 && gitw(&w.0))
            && seg
                .iter()
                .enumerate()
                .any(|(i, w)| (w.2 || w.0.contains('=')) && !is_prose(i, &w.0) && names_fetch(&w.0, i))
        {
            return Some(FETCH_PLAIN);
        }
        let assign = |w: &&(String, bool, bool)| w.0.contains('=') && !w.0.starts_with('-');
        let cmd = seg
            .iter()
            .find(|w| !assign(w) && !WRAPPERS.contains(&&*w.0));
        let cmd_glob = cmd.is_some_and(|(c, g, _)| *g && c != "[" && c != "[[");
        (cmd_glob && top || git && seg.iter().any(|w| w.1)).then_some(GLOB_WORD)
    })
}

/// Git config a plain fetch or pull follows (spec §4), `key` as `git config` prints it: a
/// refspec that moves the default branch, mirrors or writes tags from an untrusted remote, a
/// command it runs (`uploadpack`, `receivepack`, `core.sshCommand`, an `ext::`/`fd::` URL,
/// `protocol.allow` or `protocol.ext|fd.allow` not `never`), `tagOpt=--tags` on an untrusted
/// remote, or a URL rewrite to `ext::`, `fd::` or a non-network URL.
pub fn config_fetch_risk(key: &str, value: &str, default_branch: &str) -> bool {
    let k = cfg_key(key);
    let remote = k.strip_prefix("remote.").and_then(|r| r.rsplit_once('.'));
    let remote = remote.map_or("", |r| r.0);
    let trusted = matches!(remote, "origin" | "upstream");
    let v = value.to_ascii_lowercase();
    match k.rsplit('.').next().unwrap_or("") {
        "fetch" if k.starts_with("remote.") => {
            fetch_moves(&[remote, value], default_branch)
                || !trusted && norm_ref(value).contains("refs/tags/")
        }
        "uploadpack" | "receivepack" | "sshcommand" => true,
        "url" | "pushurl" => v.contains("ext::") || v.contains("fd::"),
        "tagopt" => !trusted && v.contains("--tags"),
        "insteadof" | "pushinsteadof" => {
            retargets(&k, value) || k.contains("ext::") || k.contains("fd::")
        }
        "allow" => {
            v != "never" && (k == "protocol.allow" || k.contains(".ext.") || k.contains(".fd."))
        }
        _ => false,
    }
}

/// A `remote.<r>.fetch` value that makes a later fetch move the default branch or write a tag.
fn fetch_cfg_moves(v: &str, default_branch: &str) -> bool {
    // Bash brace-expands an assigned value (`+feat:{x,main}`): fail closed.
    if v.contains('{') {
        return true;
    }
    let v = v.replace(['\'', '"'], "");
    let dst = norm_ref(v.rsplit(':').next().unwrap_or(&v));
    fetch_moves(&[".", &v], default_branch)
        || norm_ref(&v).contains("refs/tags/")
        || dst.starts_with("tags/")
}

/// A remote URL on a network host: `scheme://..` (not `file://`) or scp-like `host:path` (no
/// `/` before the `:`). Anything else (`evil2`, `../x`, `/tmp/x`) may be a local repository.
fn network_url(v: &str) -> bool {
    !local_word(v)
        && !v.contains(['{', '`'])
        && (v.contains("://")
            || v.split_once(':')
                .is_some_and(|(h, _)| !h.is_empty() && !h.contains('/')))
}

/// `git remote add|set-url origin|upstream <url>` with a URL that is not on a network host, and
/// `git remote rename <r> origin|upstream`: the trusted names must not point at a local repository.
fn remote_retargets(rest: &[&str]) -> bool {
    // Option values (`-t main`, `--tr main`, `-ft main`) are not skipped: a trusted name
    // anywhere, followed by a URL that is not on a network host, is refused.
    let words: Vec<String> = rest
        .iter()
        .map(|a| a.replace(['\'', '"'], ""))
        .filter(|a| !a.starts_with('-'))
        .collect();
    let trusted = |w: &String| matches!(w.as_str(), "origin" | "upstream");
    match words.first().map(String::as_str) {
        // A push URL does not change where a fetch reads.
        Some("set-url") if rest.iter().any(|a| a.len() >= 4 && "--push".starts_with(a)) => false,
        Some("add" | "set-url") => words
            .iter()
            .position(trusted)
            .is_some_and(|p| words[p + 1..].iter().any(|w| !network_url(w))),
        // `rename origin upstream` keeps a trusted URL; `rename evil upstream` does not.
        Some("rename") => {
            (words.last().is_some_and(trusted) && !words.get(1).is_some_and(trusted))
                || words.iter().any(|w| w.contains('$'))
        }
        _ => false,
    }
}

/// The value of `git remote --mirror` in any prefix (`--mi`), unquoted; `Some("")` when bare.
fn mirror(rest: &[&str]) -> Option<String> {
    rest.iter().find_map(|a| {
        let a = a.replace(['\'', '"'], "");
        let (k, v) = a.split_once('=').unwrap_or((&a, ""));
        (k.len() >= 4 && "--mirror".starts_with(k)).then(|| v.to_string())
    })
}

/// `--tags`/`--prune-tags` in any unique prefix, or `-t`/`-P` in a short cluster: every tag.
fn fetch_tags(rest: &[&str]) -> bool {
    rest.iter().any(|a| {
        let a = a.replace(['\'', '"'], "");
        (a.len() >= 5 && "--tags".starts_with(a.as_str()))
            || (a.len() >= 9 && "--prune-tags".starts_with(a.as_str()))
            || short_flags(&a).is_some_and(|f| f.contains(['t', 'P']))
    })
}

/// Config that points `origin`/`upstream` at a non-network URL (`remote.origin.url=evil2`) or
/// rewrites URLs to one (`url./tmp/x.insteadOf`); `k` as `cfg_key` gives it.
fn retargets(k: &str, v: &str) -> bool {
    let v = v.replace(['\'', '"'], "");
    let base = k.strip_prefix("url.").and_then(|b| {
        b.strip_suffix(".insteadof")
            .or_else(|| b.strip_suffix(".pushinsteadof"))
    });
    base.is_some_and(|b| !network_url(b))
        || (matches!(k, "remote.origin.url" | "remote.upstream.url") && !network_url(&v))
}

/// `git fetch <remote> <src>:<dst>` moves local refs like `update-ref`: refused for `--stdin`, a
/// backslash, a refspec in a variable, a glob destination that can match the default branch, or
/// a default-branch destination unless `origin`/`upstream` sends its own default branch.
fn fetch_moves(rest: &[&str], default_branch: &str) -> bool {
    if rest
        .iter()
        .any(|a| a.contains('\\') || a.starts_with("--stdin"))
    {
        return true;
    }
    let (words, maps) = fetch_words(rest);
    // A path anywhere (an option the guard misreads may hide the remote), or a `/` before the
    // first `:` of the remote word, makes the fetch local.
    let local = words.iter().any(|w| local_word(w))
        || words
            .first()
            .is_some_and(|w| w.split(':').next().unwrap_or(w).contains('/'));
    let trusted = !local && fetch_trusted(rest);
    words.iter().skip(1).chain(&maps).any(|r| {
        let spec = r.trim_start_matches('+');
        let Some((src, dst)) = spec.rsplit_once(':') else {
            return r.contains(['$', '`']);
        };
        // A glob destination that can match the default branch (`refs/heads/*`, `refs/*`, `*`).
        let dst = &norm_ref(dst);
        let glob = dst.split_once('*').is_some_and(|(pre, post)| {
            let full = format!("refs/heads/{default_branch}");
            post.contains('*')
                || [full.as_str(), &full[5..], default_branch].iter().any(|t| {
                    t.len() >= pre.len() + post.len() && t.starts_with(pre) && t.ends_with(post)
                })
        });
        r.contains(['$', '`'])
            || glob
            // `refs/heads/MAIN` is `main` on a case-insensitive file system.
            || (names_default(&dst.to_ascii_lowercase(), &default_branch.to_ascii_lowercase())
                && (!trusted || !names_default(src, default_branch)))
    })
}

/// Whether `git update-ref`, `symbolic-ref`, `branch -f|-M|-C` (or `--force` with `--copy` or
/// `--move`), `checkout -B` or `switch -C` writes the default branch (`update-ref`: any word
/// naming it, as quoted messages split); `branch -f`, `-B` and `-C` may move it to
/// `origin/<default>`, `upstream/<default>` or `refs/remotes/<r>/<default>`.
fn moves_default(sub: &str, rest: &[&str], default_branch: &str) -> bool {
    let words: Vec<String> = rest.iter().map(|a| a.replace(['\'', '"'], "")).collect();
    let short = |a: &str, fl: &[char]| !a.starts_with("--") && a.starts_with('-') && a.contains(fl);
    let named = |w: Option<&String>| w.is_some_and(|w| names_default(w, default_branch));
    let plain = words.iter().filter(|w| !w.starts_with('-'));
    // git takes a long option by a unique prefix: `--forc`, `--cop`, `--mo`.
    let long = |l: &str, min: usize| {
        let key = |w: &String| w.split('=').next().unwrap_or(w).to_string();
        words
            .iter()
            .any(|w| key(w).len() >= min && l.starts_with(&key(w)))
    };
    // A start point that is the default branch's remote-tracking ref only catches it up.
    let tracking = |w: Option<&String>| {
        w.is_some_and(|w| {
            let remote = w
                .strip_prefix("refs/remotes/")
                .and_then(|t| t.split_once('/'));
            remote.is_some_and(|(_, b)| b == default_branch)
                || w.strip_prefix("origin/")
                    .or_else(|| w.strip_prefix("upstream/"))
                    == Some(default_branch)
        })
    };
    match sub {
        "update-ref" => words.iter().any(|w| names_default(w, default_branch)),
        // Its name: the first plain word that is not the `-m` message.
        "symbolic-ref" => named(words.iter().enumerate().find_map(|(i, w)| {
            (!w.starts_with('-') && (i == 0 || words[i - 1] != "-m")).then_some(w)
        })),
        "branch" => {
            let f = long("--force", 6) || words.iter().any(|w| short(w, &['f']));
            let mc = words.iter().any(|w| short(w, &['M', 'C']))
                || (f
                    && (long("--copy", 5)
                        || long("--move", 4)
                        || words.iter().any(|w| short(w, &['c', 'm']))));
            let names: Vec<&String> = plain.collect();
            (f && named(names.first().copied()) && !tracking(names.get(1).copied()))
                || (mc && named(names.last().copied()))
        }
        // `-B <name> [<start>]`, `-Bmain`, `--force-create[=]<name>`.
        "checkout" | "switch" => words.iter().enumerate().any(|(i, w)| {
            let letter = if sub == "checkout" { 'B' } else { 'C' };
            let key = w.split('=').next().unwrap_or(w);
            let stuck = if key.len() > 7 && "--force-create".starts_with(key) {
                w.split_once('=').map(|(_, v)| v)
            } else if short(w, &[letter]) {
                w.split_once(letter).map(|(_, v)| v)
            } else {
                return false;
            };
            let (value, start) = match stuck.filter(|v| !v.is_empty()) {
                Some(v) => (Some(v.to_string()), words.get(i + 1)),
                None => (words.get(i + 1).cloned(), words.get(i + 2)),
            };
            named(value.as_ref()) && !tracking(start)
        }),
        _ => false,
    }
}

/// `v1.2.3`, `1.2` ...: a ref that looks like a version tag.
fn version_like(r: &str) -> bool {
    let t = r.strip_prefix('v').unwrap_or(r);
    // `v1`: whether a local tag of that name exists cannot be read here (core is pure), so a
    // bare `v<digits>` counts too.
    if r.starts_with('v') && !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit()) {
        return true;
    }
    t.split_once('.').is_some_and(|(a, b)| {
        !a.is_empty()
            && a.bytes().all(|c| c.is_ascii_digit())
            && b.starts_with(|c: char| c.is_ascii_digit())
    })
}

/// Whether `text` ends inside a single- or double-quoted string (`\` escapes outside `'`).
fn in_quotes(text: &str) -> bool {
    let (mut q, mut esc) = (None, false);
    for c in text.chars() {
        match (q, c) {
            _ if std::mem::take(&mut esc) => {}
            (Some('\''), '\'') => q = None,
            (Some('\''), _) => {}
            (_, '\\') => esc = true,
            (Some('"'), '"') => q = None,
            (None, '\'' | '"') => q = Some(c),
            _ => {}
        }
    }
    q.is_some()
}

/// The words of a `git push` without redirections (`> f`, `2>/dev/null`, the `2>` that
/// `2>&1` leaves once `&` splits it, `< f`) and the `)` that ends a group.
fn push_words(args: &[String]) -> Vec<&str> {
    let (mut out, mut skip) = (Vec::new(), false);
    for a in args {
        let a = a.trim_end_matches(')');
        if std::mem::take(&mut skip) {
            continue;
        }
        let op = a.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
        // A redirection glued to a word (`main>/dev/null`, `main<x`) ends it.
        if let Some(i) = op.find(['>', '<']).filter(|&i| i > 0) {
            let at = a.len() - op.len() + i;
            out.push(&a[..at]);
            skip = a[at..].trim_start_matches(['>', '<', '&']).is_empty();
        } else if let Some(target) = [">>", ">&", ">", "<"]
            .iter()
            .find_map(|o| op.strip_prefix(o))
        {
            skip = target.is_empty();
        } else if !a.is_empty() {
            out.push(a);
        }
    }
    out
}

/// `git push` flags whose value is the next word.
const PUSH_VALUE_FLAGS: [&str; 5] = ["-o", "--push-option", "--receive-pack", "--exec", "--repo"];

fn git_rule(args: &[&str], target: &Target) -> Option<&'static str> {
    let default_branch = target.default_branch;
    // git's global options: `-C dir`, `--git-dir`, `--work-tree` move the target; `-c k=v`,
    // `--no-pager` and the like do not. A target that cannot be read literally is unknown.
    let (mut i, mut opts, mut known) = (0, Vec::new(), true);
    // `-c remote.<r>.push=..` and the like; `push.default` when set.
    let (mut push_cfg, mut push_default) = (false, None);
    // `-c include.path=f` pulls in a config file a fetch or pull would follow.
    let mut include_cfg = false;
    while i < args.len() && args[i].starts_with('-') {
        let (a, next) = (args[i], args.get(i + 1).copied());
        let config = match a {
            "-c" | "--config-env" => next,
            _ => a.strip_prefix("--config-env="),
        };
        if config.is_some_and(alias_key) {
            return Some(GIT_ALIAS);
        }
        if let Some((k, v)) = config.map(|c| c.split_once('=').unwrap_or((c, ""))) {
            push_cfg |= push_key(k);
            // Bash drops a backslash before git reads the key (`remote.origin.ur\\l`).
            let fk = cfg_key(k).replace('\\', "");
            include_cfg |= fk == "include.path" || fk.starts_with("includeif.");
            if fk.starts_with("remote.")
                && fk.ends_with(".fetch")
                && (a.starts_with("--config-env") || fetch_cfg_moves(v, default_branch))
                || retargets(&fk, if a.starts_with("--config-env") { "" } else { v })
            {
                return Some(FETCH_MOVES);
            }
            if cfg_key(k) == "push.default" {
                // `--config-env` names a variable, not the value.
                let env = a.starts_with("--config-env");
                push_default = Some(if env { "" } else { v });
            }
        }
        let (flag, value, step) = match a.split_once('=') {
            Some((k @ ("--git-dir" | "--work-tree"), v)) => (Some(format!("{k}=")), Some(v), 1),
            _ => match a {
                "-C" => (Some("-C".to_string()), next, 2),
                "--git-dir" | "--work-tree" => (Some(format!("{a}=")), next, 2),
                "-c" | "--config-env" | "--namespace" | "--attr-source" | "--super-prefix" => {
                    (None, None, 2)
                }
                _ => (None, None, 1),
            },
        };
        if let Some(f) = flag {
            match value.and_then(literal_path) {
                Some(v) if f == "-C" => opts.extend([f, v]),
                Some(v) => opts.push(format!("{f}{v}")),
                None => known = false,
            }
        }
        i += step;
    }
    let sub = args.get(i)?.replace(['\'', '"', '\\'], "");
    let (sub, rest) = (sub.as_str(), &args[(i + 1).min(args.len())..]);
    if include_cfg && matches!(sub, "fetch" | "pull") {
        return Some(FETCH_MOVES);
    }
    // `git $g`, `git "${G}"`: the subcommand could be `merge`, `push` or `tag`: fail closed.
    if sub.contains(['$', '`']) {
        return Some(BUILT_SUB);
    }
    let at: Option<Vec<String>> = target
        .dir
        .filter(|_| known)
        .map(|d| d.iter().cloned().chain(opts).collect());
    let on_default = match &at {
        None => true,
        Some(all) => {
            match (target.branch_at)(all) {
                Some(b) => b == default_branch,
                // A moved target git cannot read (a failed `cd`, a missing `-C` dir) may
                // leave git where the shell started: fail closed.
                None => !all.is_empty(),
            }
        }
    };
    match sub {
        "push" | "send-pack" => {
            if push_cfg {
                return Some(PUSH_CONFIG);
            }
            // Quotes do not hide a target; one the shell expands cannot be read: fail closed.
            if rest.iter().any(|a| a.contains(['$', '`', '\\'])) {
                return Some(UNREAD_PUSH);
            }
            let unquoted: Vec<String> = rest.iter().map(|a| a.replace(['\'', '"'], "")).collect();
            let rest = push_words(&unquoted);
            // git takes a long option by any unique prefix (`--mir` is `--mirror`).
            const BULK: [&str; 9] = [
                "--force",
                "--force-with-lease",
                "--force-if-includes",
                "--tags",
                "--follow-tags",
                "--mirror",
                "--delete",
                "--all",
                "--stdin",
            ];
            if rest.iter().any(|a| {
                let long = a.split('=').next().unwrap_or(a);
                long.len() > 2 && long.starts_with("--") && BULK.iter().any(|b| b.starts_with(long))
            }) {
                return Some("force, tag, delete or bulk push");
            }
            if rest.iter().any(|a| {
                matches!(
                    *a,
                    "-f" | "--force" | "--tags" | "--follow-tags" | "--mirror" | "--delete" | "-d"
                ) || a.starts_with("--force-with-lease")
                    || a.starts_with("--force-if-includes")
                    // `-fu`, `-uf`, `-ud`: a short-flag cluster holding `f` or `d`.
                    || (a.starts_with('-') && !a.starts_with("--") && a.contains(['f', 'd']))
            }) {
                return Some("force, tag or delete push");
            }
            // Every branch, or a glob refspec: it can reach the default branch or tags.
            if rest
                .iter()
                .any(|a| matches!(*a, "--all" | "--branches") || a.contains('*'))
            {
                return Some("push of every branch or a glob refspec");
            }
            // The remote, then refspecs; the value of a flag that takes one is neither.
            let mut words = Vec::new();
            let mut it = rest.iter().copied();
            while let Some(a) = it.next() {
                if PUSH_VALUE_FLAGS.contains(&a) {
                    it.next();
                } else if !a.starts_with('-') {
                    words.push(a);
                }
            }
            let refs: Vec<&str> = words.into_iter().skip(1).collect();
            // `git push origin tag rel` and `tags/rel` push the tag `rel`.
            if refs.first() == Some(&"tag")
                || refs.iter().any(|r| {
                    r.split(':')
                        .any(|x| x.trim_start_matches('+').starts_with("tags/"))
                })
            {
                return Some("force, tag or delete push");
            }
            // `send-pack` without refs pushes every matching branch; `--stdin` reads them.
            if sub == "send-pack" && (refs.is_empty() || rest.contains(&"--stdin")) {
                return Some(UNREAD_PUSH);
            }
            if refs.is_empty() && push_default.is_some_and(|d| !head_only(d)) {
                return Some(PUSH_CONFIG);
            }
            if refs
                .iter()
                .any(|r| r.starts_with('+') || r.starts_with(':') || r.contains("refs/tags/"))
                || refs.iter().any(|r| r.split(':').any(version_like))
            {
                return Some("force, tag or delete push");
            }
            let to_default = refs.iter().any(|r| {
                let dst = r.rsplit(':').next().unwrap_or(r);
                names_default(dst, default_branch) || (matches!(dst, "HEAD" | "@") && on_default)
            });
            if to_default || (refs.is_empty() && on_default) {
                return Some("push to the default branch");
            }
            // A refspec without `:dst` (or none) follows `remote.<r>.push` and `push.default`,
            // which an earlier command may change: only alone does the hook read that config.
            let explicit = !refs.is_empty()
                && refs.iter().all(|r| {
                    r.split_once(':')
                        .is_some_and(|(s, d)| !s.is_empty() && !d.is_empty())
                });
            // `pushes` lists it either way; `tool_rules` refuses it first.
            if let Some(sink) = target.sink {
                let srcs = refs.iter().map(|r| r.split(':').next().unwrap_or(r));
                sink(Push {
                    dir: at,
                    srcs: srcs.map(str::to_string).collect(),
                });
            }
            (sub == "push" && !explicit && !target.alone).then_some(PUSH_ALONE)
        }
        "update-ref" | "symbolic-ref" | "branch" | "checkout" | "switch"
            if moves_default(sub, rest, default_branch) =>
        {
            Some("moving the default branch")
        }
        // `--stdin`, a variable, a backtick or an escape in a ref hides it: fail closed. `-m`
        // quoting is not parsed (`-m "a b"c $r`, `-dm x $r` hide the ref from any parser), so ANY
        // non-flag argument with `$` or a backtick denies, a `"$sha"` value included.
        // `symbolic-ref` writes a ref the same way (`refs/tags/x` or `tags/x` as its name).
        "update-ref" | "symbolic-ref" => (rest.iter().any(|a| {
            let q = norm_ref(&a.replace(['\'', '"'], ""));
            a.contains(['`', '\\'])
                || a.starts_with("--stdin")
                || q.contains("refs/tags/")
                || (sub == "symbolic-ref" && q.starts_with("tags/"))
                || (!q.starts_with('-') && q.contains('$'))
        }))
        .then_some("tag"),
        // `git fetch . HEAD:refs/tags/v1` makes a tag too; mirroring the remote's own tags
        // (`'refs/tags/*:refs/tags/*'`, as `--tags`) does not.
        // `git pull <remote> <src>:<dst>` hands its refspecs to fetch.
        "fetch" | "pull"
            if (fetch_tags(rest) && !fetch_trusted(rest))
                // `tag <name>` is git's shorthand for `refs/tags/<name>:refs/tags/<name>`.
                || fetch_words(rest).0.iter().skip(1).any(|w| w.trim_start_matches('+') == "tag")
                || rest.iter().any(|a| {
                let a = norm_ref(&a.replace(['\'', '"', '\\'], ""));
                a.contains("refs/tags/")
                    && (a.trim_start_matches('+') != "refs/tags/*:refs/tags/*" || !fetch_trusted(rest))
                    // `<src>:$T` writes a ref the hook cannot read; `<src>:tags/x` is a DWIM tag.
                    || a.rsplit_once(':').is_some_and(|(_, d)| d.contains(['$', '`']) || d.starts_with("tags/"))
            }) =>
        {
            Some("tag")
        }
        "fetch" | "pull" if fetch_moves(rest, default_branch) => Some(FETCH_MOVES),
        "tag" => {
            let lists = rest.is_empty()
                || rest
                    .iter()
                    .any(|a| matches!(*a, "-l" | "--list" | "-v" | "--verify"))
                || rest.iter().all(|a| {
                    a.starts_with("-n")
                        || a.starts_with("--contains")
                        || a.starts_with("--points-at")
                });
            (!lists).then_some("tag")
        }
        "subtree" if rest.first() == Some(&"push") => subtree_push(&rest[1..], default_branch),
        "config" => {
            let reads = matches!(rest.first(), Some(&("get" | "list")))
                || rest.iter().any(|a| {
                    matches!(*a, "--get" | "--get-all" | "--get-regexp" | "-l" | "--list")
                });
            if reads {
                return None;
            }
            if rest.iter().any(|a| alias_key(a)) {
                return Some(GIT_ALIAS);
            }
            // Writing push targets into config; unsetting them is fine.
            let unsets = rest
                .iter()
                .any(|a| matches!(*a, "--unset" | "--unset-all" | "unset"));
            // A key with a value after it is a write; the key alone reads it.
            let value = |i: usize| rest.get(i + 1).filter(|v| !v.starts_with('-'));
            let writes = rest.iter().enumerate().any(|(i, a)| {
                (push_key(a) && value(i).is_some())
                    || (cfg_key(a) == "push.default" && value(i).is_some_and(|v| !head_only(v)))
            });
            // `remote.<r>.fetch` (also `--add`, `--replace-all`, `set`) that a later fetch follows.
            let fetch = rest.iter().enumerate().any(|(i, a)| {
                let k = cfg_key(a).replace('\\', "");
                k.starts_with("remote.")
                    && k.ends_with(".fetch")
                    && value(i).is_some_and(|v| fetch_cfg_moves(v, default_branch))
                    || value(i).is_some_and(|v| retargets(&k, v))
            });
            // `--rename-section remote.evil remote.upstream` renames a remote like `git remote rename`.
            let plain: Vec<String> = rest
                .iter()
                .filter(|a| !a.starts_with('-') || a.starts_with("--rename-section"))
                .map(|a| cfg_key(a))
                .collect();
            let renames = plain
                .first()
                .is_some_and(|w| matches!(w.as_str(), "rename-section" | "--rename-section"));
            let trusted = |w: Option<&String>| {
                w.is_some_and(|w| matches!(w.as_str(), "remote.origin" | "remote.upstream"))
            };
            if renames && trusted(plain.last()) && !trusted(plain.get(1).filter(|_| plain.len() > 2))
            {
                return Some(FETCH_MOVES);
            }
            if !unsets && fetch {
                return Some(FETCH_MOVES);
            }
            (!unsets && writes).then_some(PUSH_CONFIG)
        }
        // `--mirror=push` (or a bare `--mirror`) makes every push a mirror push.
        // `--mirror=fetch` writes `+refs/*:refs/*`, which a later bare fetch follows.
        // Any unique prefix of `--mirror`, quoted or not (`--mi=fetch`, `'--mirror=fetch'`).
        "remote" if mirror(rest).is_some_and(|v| !v.is_empty() && "fetch".starts_with(&v)) => {
            Some(FETCH_MOVES)
        }
        "remote" if remote_retargets(rest) => Some(FETCH_MOVES),
        "remote" => mirror(rest).is_some().then_some(PUSH_CONFIG),
        "merge" if ff_only(rest, default_branch) => None,
        "merge" | "rebase" | "cherry-pick" | "reset" | "commit" if on_default => {
            Some("changing the default branch")
        }
        _ => None,
    }
}
