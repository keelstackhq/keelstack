//! The quorum trust root is a rule plus this check (D-022), not a hand-kept list that keeps
//! missing paths: (a) every `.claude/**` file and `.mcp.json` at any depth; (b) every file that a
//! privileged workflow job, a hook, a SessionStart command, a setup script or a channel template
//! read at the base reads or executes. This test walks the repo, extracts those inputs and asserts
//! each one is `trust_root_changed` under the code root `bosun`. It also pins that no job holding a
//! secret or an id-token restores a cache. Every walk asserts a minimum count, so a parser that
//! stops finding things fails instead of passing vacuously.

use bosun::core::risk::quorum_trust_paths;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(path: impl AsRef<Path>) -> String {
    let p = root().join(path.as_ref());
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The set risk-check and the carry both refuse to count a quorum on (D-022).
fn is_trust_root(path: &str) -> bool {
    !quorum_trust_paths(&[(path, None)], &["bosun".to_string()]).is_empty()
}

/// Repo-relative path of every file below the repo root whose name matches `keep`, skipping
/// build output and dependency folders.
fn walk(keep: &dyn Fn(&str) -> bool) -> Vec<String> {
    fn go(dir: &Path, rel: &str, keep: &dyn Fn(&str) -> bool, out: &mut Vec<String>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if matches!(name.as_str(), "node_modules" | "target" | ".git" | "dist") {
                continue;
            }
            let r = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            match e.file_type() {
                Ok(t) if t.is_dir() => go(&e.path(), &r, keep, out),
                Ok(t) if t.is_file() && keep(&r) => out.push(r),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    go(&root(), "", keep, &mut out);
    out.sort();
    out
}

/// `a/b/../c` and `./x` as a plain repo-relative path.
fn norm(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for s in path.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn join(base: &str, rel: &str) -> String {
    if base.is_empty() {
        norm(rel)
    } else {
        norm(&format!("{base}/{rel}"))
    }
}

fn is_file(rel: &str) -> bool {
    root().join(rel).is_file()
}

const ROOT_MARK: &str = "@root@";

/// Where a shell text runs. `base`: its start directory (repo-relative); `strict`: a bare path
/// resolves against the working directory only (a `run:` step does, with no root fallback);
/// `action`: the directory `$GITHUB_ACTION_PATH` names; `tools`: whether `pnpm` and `cargo` words
/// count as running them.
struct Cx<'a> {
    base: &'a str,
    action: Option<&'a str>,
    strict: bool,
    tools: bool,
}

fn indent(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

fn unquote(v: &str) -> String {
    let v = v.split(" #").next().unwrap_or("").trim();
    v.trim_matches(|c| c == '"' || c == '\'').to_string()
}

/// A path as the workspace root: `$GITHUB_WORKSPACE`, `${GITHUB_WORKSPACE}` and
/// `${{ github.workspace }}` name the repo root.
fn root_vars(text: &str) -> String {
    text.replace("\"${GITHUB_WORKSPACE}\"", ROOT_MARK)
        .replace("\"$GITHUB_WORKSPACE\"", ROOT_MARK)
        .replace("${GITHUB_WORKSPACE}", ROOT_MARK)
        .replace("$GITHUB_WORKSPACE", ROOT_MARK)
        .replace("${{ github.workspace }}", ROOT_MARK)
}

fn resolve(cx: &Cx, cwd: &str, d: &str) -> String {
    if let Some(r) = d.strip_prefix(ROOT_MARK) {
        norm(r)
    } else if d.starts_with('/') {
        join(cx.base, d.trim_start_matches('/'))
    } else {
        join(cwd, d)
    }
}

/// What a shell text (a `run:` block, a hook command, a setup script, a script it runs) runs or
/// reads, as repo-relative paths, with the origin of each:
/// - every token that resolves to an existing file (a script, a pins file, a lockfile, ...), a
///   `.md` file only on a line that reads it (`cat x.md`, `< x.md`, `--body-file x.md`);
/// - `cd X`, `pnpm --dir X`, `pnpm -C X`, `working-directory: X`: when pnpm is used there,
///   `X/package.json`, `X/pnpm-lock.yaml`, `X/pnpm-workspace.yaml`, `X/.pnpmfile.cjs`;
/// - `cargo`: `Cargo.toml`, `build.rs` and `.cargo/config.toml` of the start directory.
fn inputs_in(text: &str, cx: &Cx, origin: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut add = |p: String, why: &str| {
        out.entry(p).or_insert_with(|| format!("{origin}: {why}"));
    };
    let mut text: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("${CLAUDE_PROJECT_DIR}", "")
        .replace("$CLAUDE_PROJECT_DIR", "");
    text = root_vars(&text);
    if let Some(a) = cx.action {
        let m = if a.is_empty() {
            ROOT_MARK.to_string()
        } else {
            format!("{ROOT_MARK}/{a}")
        };
        text = text
            .replace("${GITHUB_ACTION_PATH}", &m)
            .replace("$GITHUB_ACTION_PATH", &m)
            .replace("${{ github.action_path }}", &m);
    }
    let sep = Regex::new(r#"[\s"'`();&|<>,\[\]]+"#).unwrap();
    let reader = Regex::new(
        r"(?:^|[\s(])(?:cat|head|tail|sed|awk|grep|rg|jq|wc|cp|mv|diff|tee|xargs|readarray|mapfile|source|node|python3?|bash|sh)(?:\s|$)|<|--(?:body-|notes-)?file|\s-F\s",
    )
    .unwrap();
    let echoish = Regex::new(r"\b(?:echo|printf)\b|::(?:error|notice|warning)").unwrap();
    let mut cwd = cx.base.to_string();
    let mut dirs: Vec<String> = vec![cx.base.to_string()];
    let (mut pnpm, mut cargo) = (false, false);
    for line in text.lines() {
        let reads = reader.is_match(line) && !echoish.is_match(line);
        let toks: Vec<&str> = sep.split(line).filter(|t| !t.is_empty()).collect();
        for (i, tok) in toks.iter().enumerate() {
            let t = tok.trim_end_matches(':');
            let next = toks.get(i + 1).copied();
            match (t, next) {
                ("cd", Some(d)) if !d.contains('$') => {
                    cwd = resolve(cx, &cwd, d);
                    dirs.push(cwd.clone());
                }
                ("working-directory", Some(d)) if !d.contains('$') && !d.contains("{{") => {
                    dirs.push(resolve(cx, cx.base, d));
                }
                ("--dir" | "-C", Some(d)) if !d.contains('$') => dirs.push(resolve(cx, &cwd, d)),
                ("pnpm", _) if cx.tools => pnpm = true,
                ("cargo", _) if cx.tools => cargo = true,
                _ => {}
            }
            // Docs (`.github/SETUP.md` in an error message) are text, not an input.
            if t.contains('$')
                || t.contains("{{")
                || t.starts_with('-')
                || t.contains("://")
                || (t.ends_with(".md") && !reads)
            {
                continue;
            }
            let cands: Vec<String> = if let Some(r) = t.strip_prefix(ROOT_MARK) {
                vec![norm(r)]
            } else {
                let t = t.trim_start_matches("./").trim_start_matches('/');
                if cx.strict {
                    vec![join(&cwd, t)]
                } else {
                    vec![join(&cwd, t), join(cx.base, t), norm(t)]
                }
            };
            for cand in cands {
                // `uses: ./keelstack/hoard` names a second checkout of this repo.
                let cand = if is_file(&cand) {
                    cand
                } else if let Some(c) = cand.strip_prefix("keelstack/").filter(|c| is_file(c)) {
                    c.to_string()
                } else {
                    continue;
                };
                add(cand, "reads or runs it");
                break;
            }
        }
    }
    if pnpm {
        for d in &dirs {
            if is_file(&join(d, "package.json")) {
                for f in [
                    "package.json",
                    "pnpm-lock.yaml",
                    "pnpm-workspace.yaml",
                    ".pnpmfile.cjs",
                ] {
                    add(join(d, f), "pnpm loads it");
                }
            }
        }
    }
    if cargo {
        for d in &dirs {
            for f in ["Cargo.toml", "build.rs", ".cargo/config.toml"] {
                add(join(d, f), "cargo builds with it");
            }
        }
    }
    out
}

fn inputs(text: &str, base: &str, origin: &str) -> BTreeMap<String, String> {
    let cx = Cx {
        base,
        action: None,
        strict: false,
        tools: true,
    };
    inputs_in(text, &cx, origin)
}

fn merge(into: &mut BTreeMap<String, String>, from: BTreeMap<String, String>) {
    for (k, v) in from {
        into.entry(k).or_insert(v);
    }
}

/// `(text outside the steps, the steps)` of the first `steps:` list of a job block or an action
/// file, each step as its own text.
fn steps_of(yaml: &str) -> (String, Vec<String>) {
    let lines: Vec<&str> = yaml.lines().collect();
    let Some(k) = lines.iter().position(|l| l.trim() == "steps:") else {
        return (yaml.to_string(), Vec::new());
    };
    let key = indent(lines[k]);
    let mut outside: Vec<&str> = lines[..k].to_vec();
    let mut steps: Vec<Vec<&str>> = Vec::new();
    let mut item: Option<usize> = None;
    for (n, l) in lines.iter().enumerate().skip(k + 1) {
        let t = l.trim_start();
        if t.is_empty() || t.starts_with('#') {
            if let Some(s) = steps.last_mut() {
                s.push(l);
            }
            continue;
        }
        let ind = indent(l);
        if ind < key || (ind == key && !t.starts_with("- ")) {
            outside.extend(&lines[n..]);
            break;
        }
        let it = *item.get_or_insert(ind);
        if ind == it && t.starts_with("- ") {
            steps.push(vec![l]);
        } else if let Some(s) = steps.last_mut() {
            s.push(l);
        }
    }
    (
        outside.join("\n"),
        steps.into_iter().map(|s| s.join("\n")).collect(),
    )
}

/// `(column, text)` of each key line of a step: the first line is `- key: v`.
fn step_keys(step: &str) -> Vec<(usize, &str, usize)> {
    let first = step.lines().next().map(indent).unwrap_or(0);
    step.lines()
        .enumerate()
        .map(|(n, l)| {
            let t = l.trim_start();
            match t.strip_prefix("- ") {
                Some(b) => (indent(l) + 2, b.trim_start(), n),
                None => (indent(l), t, n),
            }
        })
        .filter(|(col, _, _)| *col == first + 2)
        .collect()
}

/// A step-level key's value (`with:` children are deeper and never match).
fn step_key(step: &str, key: &str) -> Option<String> {
    let pre = format!("{key}:");
    step_keys(step)
        .into_iter()
        .find_map(|(_, body, _)| body.strip_prefix(&pre).map(unquote))
}

/// `(the step without its run, the run text)`.
fn split_run(step: &str) -> (String, String) {
    let lines: Vec<&str> = step.lines().collect();
    let keys = step_keys(step);
    let (mut rest, mut run) = (Vec::new(), String::new());
    let mut i = 0;
    while i < lines.len() {
        let hit = keys
            .iter()
            .find(|(_, b, n)| *n == i && b.starts_with("run:"));
        let Some((col, body, _)) = hit else {
            rest.push(lines[i]);
            i += 1;
            continue;
        };
        let v = body["run:".len()..].trim();
        i += 1;
        if v.starts_with('|') || v.starts_with('>') {
            while i < lines.len() && (lines[i].trim().is_empty() || indent(lines[i]) > *col) {
                run.push_str(lines[i]);
                run.push('\n');
                i += 1;
            }
        } else {
            run.push_str(&unquote(v));
            run.push('\n');
        }
    }
    (rest.join("\n"), run)
}

/// A `working-directory:` value as a repo-relative path (relative values are workspace-relative);
/// `None` when it is dynamic.
fn clean_wd(v: &str) -> Option<String> {
    let v = root_vars(&unquote(v));
    if v.contains('$') || v.contains("{{") {
        return None;
    }
    Some(norm(v.strip_prefix(ROOT_MARK).unwrap_or(&v)))
}

fn default_wd(text: &str) -> Option<String> {
    let re = Regex::new(r"(?m)^\s*working-directory:\s*(.+)$").unwrap();
    let code = code_lines(text).join("\n");
    re.captures(&code).and_then(|c| clean_wd(&c[1]))
}

/// The repo directory a `uses: ./X` step names (`./keelstack/X` is a second checkout).
fn local_action(uses: &str) -> Option<String> {
    let p = norm(uses.strip_prefix("./")?);
    let p = match p.strip_prefix("keelstack/") {
        Some(c) if !root().join(&p).is_dir() => c.to_string(),
        _ => p,
    };
    root().join(&p).is_dir().then_some(p)
}

/// Files a local action runs: its `action.yml`, the `main`, `pre` and `post` files and a Docker
/// `image` file of `runs:`, and, for a composite one, what its `run:` steps read (with the action
/// directory as `$GITHUB_ACTION_PATH`) and the local actions its steps use.
fn action_inputs(
    dir: &str,
    origin: &str,
    seen: &mut std::collections::BTreeSet<String>,
    out: &mut BTreeMap<String, String>,
) {
    if !seen.insert(dir.to_string()) {
        return;
    }
    let why = format!("{origin}: local action {dir}");
    let file = Regex::new(r#"(?m)^\s+(?:main|pre|post|image):\s*['"]?([^'"\s#]+)"#).unwrap();
    for name in ["action.yml", "action.yaml"] {
        let f = join(dir, name);
        let Ok(text) = fs::read_to_string(root().join(&f)) else {
            continue;
        };
        out.entry(f).or_insert_with(|| why.clone());
        let runs = text.split("\nruns:").nth(1).unwrap_or("");
        for c in file.captures_iter(runs) {
            if !c[1].contains("://") {
                out.entry(join(dir, &c[1])).or_insert_with(|| why.clone());
            }
        }
        for step in steps_of(&text).1 {
            let wd = step_key(&step, "working-directory").and_then(|v| clean_wd(&v));
            let (rest, run) = split_run(&step);
            let mut cx = Cx {
                base: "",
                action: Some(dir),
                strict: false,
                tools: true,
            };
            merge(out, inputs_in(&rest, &cx, &why));
            cx.base = wd.as_deref().unwrap_or("");
            cx.strict = true;
            merge(out, inputs_in(&run, &cx, &why));
            if let Some(d) = step_key(&step, "uses").and_then(|u| local_action(&u)) {
                action_inputs(&d, &why, seen, out);
            }
        }
    }
}

/// What a privileged job reads or runs: per step, a `run:` resolves against the step's
/// `working-directory:` (else the job's `defaults.run`, else the workflow's), the other text
/// against the repo root, and a `uses: ./X` brings in the action's files and its composite steps.
fn job_inputs(file: &str, job: &str, origin: &str) -> BTreeMap<String, String> {
    let top = file.split("\njobs:").next().unwrap_or("");
    let (outside, steps) = steps_of(job);
    let job_wd = default_wd(&outside)
        .or_else(|| default_wd(top))
        .unwrap_or_default();
    let mut out = inputs(&outside, "", origin);
    let mut seen = std::collections::BTreeSet::new();
    for step in steps {
        let wd = (step_key(&step, "working-directory").and_then(|v| clean_wd(&v)))
            .unwrap_or_else(|| job_wd.clone());
        let (rest, run) = split_run(&step);
        merge(&mut out, inputs(&rest, "", origin));
        let cx = Cx {
            base: &wd,
            action: None,
            strict: true,
            tools: true,
        };
        merge(&mut out, inputs_in(&run, &cx, origin));
        if let Some(d) = step_key(&step, "uses").and_then(|u| local_action(&u)) {
            action_inputs(&d, origin, &mut seen, &mut out);
        }
    }
    out
}

/// Relative `import`, `export ... from`, `require` and `import()` specifiers of a JS or TS text
/// in `dir`, as the existing files they name.
fn js_imports(text: &str, dir: &str) -> Vec<String> {
    let re = Regex::new(
        r#"(?:\bfrom\s*|\bimport\s*\(?\s*|\brequire\s*\(\s*)['"](\.{1,2}(?:/[^'"]*)?)['"]"#,
    )
    .unwrap();
    let exts = ["", ".mjs", ".js", ".cjs", ".ts", ".mts", ".cts", ".json"];
    let mut out = Vec::new();
    for c in re.captures_iter(text) {
        let base = join(dir, &c[1]);
        let cands = exts
            .iter()
            .map(|e| format!("{base}{e}"))
            .chain(exts[1..].iter().map(|e| join(&base, &format!("index{e}"))));
        if let Some(f) = cands.into_iter().find(|f| is_file(f)) {
            out.push(f);
        }
    }
    out
}

/// A shell script's own-directory variables as paths from the repo root.
fn script_vars(text: &str, dir: &str) -> String {
    let own = format!("{ROOT_MARK}/{dir}");
    let dirname =
        Regex::new(r"\$\(\s*(?:cd\s+)?[^)]*dirname[^)]*\)\)?(?:\s*&&\s*pwd\s*\))?").unwrap();
    let var =
        Regex::new(r"\$\{?(?:SCRIPT_DIR|SCRIPTDIR|SCRIPT_PATH|HERE|DIR|here|dir)\}?").unwrap();
    let t = dirname.replace_all(text, own.as_str());
    let t = var.replace_all(&t, own.as_str());
    let t = Regex::new(r#""(@root@[^"\s]*)"/"#)
        .unwrap()
        .replace_all(&t, "$1/");
    t.into_owned()
}

/// Every file `found` names, then every file those scripts read or run, until nothing new
/// shows up: JS and TS relative imports, shell `source`, `.`, `bash x.sh`, `node x.mjs`, any
/// token that is an existing file. Bounded (`MAX_CLOSED` files read) and deterministic
/// (sorted work list).
fn close(found: &mut BTreeMap<String, String>) {
    const MAX_CLOSED: usize = 600;
    let script = |p: &str| {
        let ext = p.rsplit('.').next().unwrap_or("");
        matches!(
            ext,
            "sh" | "bash" | "mjs" | "js" | "cjs" | "ts" | "mts" | "cts" | "py" | "rb"
        )
    };
    let mut done = std::collections::BTreeSet::new();
    loop {
        let todo: Vec<String> = (found.keys())
            .filter(|p| script(p) && !done.contains(*p))
            .cloned()
            .collect();
        if todo.is_empty() || done.len() >= MAX_CLOSED {
            return;
        }
        for p in todo {
            done.insert(p.clone());
            let Ok(text) = fs::read_to_string(root().join(&p)) else {
                continue;
            };
            if text.len() > 512 * 1024 {
                continue;
            }
            let dir = p.rsplit_once('/').map_or("", |(d, _)| d).to_string();
            let why = format!("read by {p}");
            let is_shell = p.ends_with(".sh") || p.ends_with(".bash");
            let cx = Cx {
                base: &dir,
                action: None,
                strict: false,
                tools: is_shell,
            };
            let text = if is_shell {
                script_vars(&text, &dir)
            } else {
                text
            };
            let mut more = inputs_in(&text, &cx, &why);
            if !is_shell {
                for f in js_imports(&text, &dir) {
                    more.entry(f).or_insert_with(|| why.clone());
                }
            }
            merge(found, more);
        }
    }
}

/// Job blocks of a workflow: `(id, text)` for each two-space-indented key under `jobs:`.
fn jobs(yaml: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_jobs = false;
    for line in yaml.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        if !line.starts_with(' ') && !line.is_empty() && !line.starts_with('#') {
            break;
        }
        if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') {
            let id = line.trim().trim_end_matches(':').to_string();
            out.push((id, String::new()));
        } else if let Some((_, t)) = out.last_mut() {
            t.push_str(line);
            t.push('\n');
        }
    }
    out
}

fn code_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect()
}

/// A secret other than the workflow's own token, an id-token, or a deployment environment: a
/// named `secrets.X`, any `secrets[...]`, a bare `secrets` or `toJSON(secrets)` expression, or
/// `secrets: inherit`.
fn holds_secret(job: &str) -> bool {
    let named = Regex::new(r"\$\{\{.*?\bsecrets\.([A-Za-z0-9_]+)").unwrap();
    let whole = Regex::new(r"\$\{\{.*?\bsecrets\b(?:\s*\[|\s*\)|\s*,|\s*\}\})").unwrap();
    let inherit = Regex::new(r"^\s*secrets:\s*inherit\b").unwrap();
    code_lines(job).iter().any(|l| {
        named.captures_iter(l).any(|c| &c[1] != "GITHUB_TOKEN")
            || whole.is_match(l)
            || inherit.is_match(l)
            || l.trim() == "id-token: write"
    })
}

/// Whether a `permissions:` key at `indent_n` spaces in `text` grants any `write`: `None` when
/// the key is absent.
fn perm_write(text: &str, indent_n: usize) -> Option<bool> {
    let lines: Vec<&str> = code_lines(text);
    let key = format!("{}permissions:", " ".repeat(indent_n));
    let k = lines.iter().position(|l| l.starts_with(&key))?;
    let v = unquote(&lines[k][key.len()..]);
    if !v.is_empty() {
        return Some(v.contains("write"));
    }
    Some(
        lines[k + 1..]
            .iter()
            .take_while(|l| l.trim().is_empty() || indent(l) > indent_n)
            .any(|l| {
                l.split_once(':')
                    .is_some_and(|(_, v)| unquote(v) == "write")
            }),
    )
}

/// The job's `GITHUB_TOKEN` can write: its own `permissions:` (else the workflow's) grants
/// any `write`, or none is declared anywhere (the repository default is then unknown).
fn writes_token(file_text: &str, job: &str) -> bool {
    let top = file_text.split("\njobs:").next().unwrap_or("");
    perm_write(job, 4).unwrap_or_else(|| perm_write(top, 0).unwrap_or(true))
}

/// A job whose files must be trust root: it holds a secret, an id-token or an environment, its
/// `GITHUB_TOKEN` can write, or its workflow runs in the base's context
/// (`pull_request_target`, `workflow_run`).
fn privileged(file_text: &str, job: &str) -> bool {
    let ctx = code_lines(file_text)
        .iter()
        .any(|l| l.contains("pull_request_target") || l.contains("workflow_run"));
    ctx || holds_secret(job)
        || writes_token(file_text, job)
        || code_lines(job)
            .iter()
            .any(|l| l.starts_with("    environment:"))
}

fn workflows() -> Vec<String> {
    walk(&|p| {
        p.starts_with(".github/workflows/") && p.ends_with(".yml") && p.matches('/').count() == 2
    })
}

#[test]
fn the_rule_covers_claude_and_mcp_at_any_depth() {
    for p in [
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".claude/agents/r.md",
        ".claude/hooks/x.sh",
        "hull/.claude/settings.json",
        "splice/.claude/settings.json",
        "hoist/.claude/settings.json",
        "hoard/.claude/settings.json",
        "berth/.claude/settings.json",
        "a/b/c/.claude/anything/deep.md",
        ".mcp.json",
        "hull/.mcp.json",
        "a/b/.mcp.json",
    ] {
        assert!(is_trust_root(p), "{p}");
    }
    for p in ["hull/.claudeX/a", "x.mcp.json", "mcp.json"] {
        assert!(!is_trust_root(p), "{p}");
    }
}

#[test]
fn what_the_session_start_hook_installs_is_trust_root() {
    for p in [
        "hull/package.json",
        "hull/pnpm-lock.yaml",
        "hull/pnpm-workspace.yaml",
        "hull/.pnpmfile.cjs",
        "hull/.pnpmfile.mjs",
        "pins.toml",
        "bosun/pins.toml",
    ] {
        assert!(is_trust_root(p), "{p}");
    }
}

#[test]
fn the_extractor_finds_what_it_must() {
    let got = inputs(
        r#"cd "$CLAUDE_PROJECT_DIR/hull" && [ -f package.json ] && pnpm install --silent; bash scripts/bump-version.sh; cargo test"#,
        "",
        "t",
    );
    for want in [
        "hull/package.json",
        "hull/pnpm-lock.yaml",
        "hull/pnpm-workspace.yaml",
        "hull/.pnpmfile.cjs",
        "scripts/bump-version.sh",
        "Cargo.toml",
        "build.rs",
    ] {
        assert!(got.contains_key(want), "{want} not in {got:?}");
    }
    let got = inputs(
        "pnpm --dir site --filter x deploy; pnpm -C hull test",
        "",
        "t",
    );
    assert!(got.contains_key("site/package.json") && got.contains_key("hull/package.json"));
    assert!(inputs("echo hi", "", "t").is_empty());
}

/// A run resolves against the `working-directory` that applies to it: the step's, else the job's
/// `defaults.run`, else the workflow's; `$GITHUB_WORKSPACE` and `${{ github.workspace }}` strip to
/// the repo root.
#[test]
fn working_directories_decide_what_a_run_reads() {
    let wf = |top: &str, job: &str, step: &str| {
        let file = format!("name: t\n{top}jobs:\n  j:\n{job}    steps:\n{step}");
        let (_, j) = jobs(&file).remove(0);
        job_inputs(&file, &j, "t")
    };
    let run = "      - run: bash scripts/verify-build-shas.sh\n";
    let want = "hoard/scripts/verify-build-shas.sh";
    // The step's own working-directory.
    let got = wf(
        "",
        "",
        "      - working-directory: hoard\n        run: bash scripts/verify-build-shas.sh\n",
    );
    assert!(got.contains_key(want), "step: {got:?}");
    assert!(!got.contains_key("scripts/bump-version.sh"));
    // The job's defaults.run, the workflow's, and the step overriding both.
    let job_defaults = "    defaults:\n      run:\n        working-directory: hoard\n";
    let got = wf("", job_defaults, run);
    assert!(got.contains_key(want), "job: {got:?}");
    let top_defaults = "defaults:\n  run:\n    working-directory: hoard\n";
    let got = wf(top_defaults, "", run);
    assert!(got.contains_key(want), "workflow: {got:?}");
    let got = wf(
        top_defaults,
        job_defaults,
        "      - working-directory: .\n        run: bash scripts/bump-version.sh\n",
    );
    assert!(
        got.contains_key("scripts/bump-version.sh"),
        "step beats job: {got:?}"
    );
    assert!(!got.contains_key("hoard/scripts/bump-version.sh"));
    // `$GITHUB_WORKSPACE` and `${{ github.workspace }}` are the repo root, inside a run and as a value.
    let got = wf(
        "",
        job_defaults,
        "      - run: bash \"$GITHUB_WORKSPACE/scripts/bump-version.sh\"\n",
    );
    assert!(got.contains_key("scripts/bump-version.sh"), "{got:?}");
    let got = wf(
        "",
        "",
        "      - run: bash ${GITHUB_WORKSPACE}/scripts/bump-version.sh\n",
    );
    assert!(got.contains_key("scripts/bump-version.sh"), "{got:?}");
    let got = wf(
        "",
        "",
        "      - working-directory: ${{ github.workspace }}/hoard\n        run: bash scripts/verify-build-shas.sh\n",
    );
    assert!(got.contains_key(want), "{got:?}");
    // pnpm in a working directory loads that directory's manifests.
    let got = wf(
        "",
        "",
        "      - working-directory: hull\n        run: pnpm install\n",
    );
    assert!(got.contains_key("hull/pnpm-lock.yaml"), "{got:?}");
    // A `with: working-directory:` input of an action is not the step's.
    let got = wf(
        "",
        "",
        "      - uses: actions/x@v1\n        with:\n          working-directory: hoard\n      - run: bash scripts/bump-version.sh\n",
    );
    assert!(got.contains_key("scripts/bump-version.sh"), "{got:?}");
}

/// `uses: ./X` in a job: the action file, its `runs.main/pre/post` files and the files its
/// composite `run:` steps read (`$GITHUB_ACTION_PATH` is the action's directory).
#[test]
fn a_local_action_brings_in_its_files() {
    let file = "name: t\njobs:\n  j:\n    steps:\n      - uses: ./hoard\n      - uses: ./splice/action\n      - uses: ./keelstack/hoist/release\n";
    let (_, j) = jobs(file).remove(0);
    let got = job_inputs(file, &j, "t");
    for want in [
        "hoard/action.yml",
        "splice/action/action.yml",
        "splice/action/args.mjs",
        "hoist/release/action.yml",
    ] {
        assert!(got.contains_key(want), "{want} not in {got:?}");
    }
    let main = fs::read_to_string(root().join("hoard/action.yml")).unwrap();
    let main = Regex::new(r"(?m)^\s+main:\s*(\S+)")
        .unwrap()
        .captures(&main)
        .unwrap()[1]
        .to_string();
    assert!(
        got.contains_key(&join("hoard", &main)),
        "main {main} not in {got:?}"
    );
}

/// What a script reads or runs is followed to a fixed point.
#[test]
fn imports_and_sourced_scripts_are_followed() {
    assert_eq!(
        js_imports(
            "import a from './summary.mjs';\nconst b = require('./args');\nexport * from \"./args.mjs\";\nimport x from 'node:fs';\nconst c = await import('../action/args.mjs')",
            "splice/action"
        ),
        [
            "splice/action/summary.mjs",
            "splice/action/args.mjs",
            "splice/action/args.mjs",
            "splice/action/args.mjs"
        ]
    );
    let sh = script_vars(
        "source \"$SCRIPT_DIR/x.sh\"\n. \"$(dirname \"$0\")/y.sh\"",
        "scripts",
    );
    assert!(
        sh.contains("@root@/scripts/x.sh") && sh.contains("@root@/scripts/y.sh"),
        "{sh}"
    );
    // A script that names another, and one that sources a third.
    let mut found = BTreeMap::new();
    found.insert("splice/scripts/smoke.sh".to_string(), "t".to_string());
    close(&mut found);
    let text = fs::read_to_string(root().join("splice/scripts/smoke.sh")).unwrap();
    let names: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || "\"'()".contains(c))
        .filter(|t| t.ends_with(".mjs") || t.ends_with(".sh"))
        .collect();
    assert!(!names.is_empty() || found.len() == 1, "{found:?}");
    // `.md` counts only where it is read.
    let md = |t: &str| inputs(t, "", "t").contains_key("bosun/CLAUDE.md");
    assert!(md("cat bosun/CLAUDE.md"));
    assert!(md("x < bosun/CLAUDE.md"));
    assert!(md("gh pr comment --body-file bosun/CLAUDE.md"));
    assert!(!md("echo \"see bosun/CLAUDE.md\""));
    assert!(!md("bosun/CLAUDE.md"));
}

/// Every `.claude/settings*.json` hook command: what it runs is trust root.
#[test]
fn every_hook_command_input_is_trust_root() {
    let settings = walk(&|p| {
        let name = p.rsplit('/').next().unwrap();
        p.contains(".claude/")
            && name.starts_with("settings")
            && name.ends_with(".json")
            // Templates that render into other repos are not this repo's hooks.
            && !p.contains("/pieces/")
    });
    assert!(settings.len() >= 6, "settings files: {settings:?}");
    let (mut commands, mut inputs_seen) = (0, 0);
    let mut missing = Vec::new();
    for s in &settings {
        assert!(is_trust_root(s), "{s}");
        let v: Value = serde_json::from_str(&read(s)).unwrap_or_else(|e| panic!("{s}: {e}"));
        let project = s.strip_suffix(".claude/settings.json").map(str::to_string);
        let project = project
            .or_else(|| s.rsplit_once(".claude/").map(|(d, _)| d.to_string()))
            .unwrap();
        let project = project.trim_end_matches('/');
        let mut cmds = Vec::new();
        collect_commands(&v["hooks"], &mut cmds);
        for c in cmds {
            commands += 1;
            let mut found = inputs(&c, project, s);
            close(&mut found);
            for (p, why) in found {
                inputs_seen += 1;
                if !is_trust_root(&p) {
                    missing.push(format!("{p} ({why})"));
                }
            }
        }
    }
    assert!(commands >= 20, "hook commands found: {commands}");
    assert!(inputs_seen >= 4, "hook inputs found: {inputs_seen}");
    // The hull SessionStart hook is the one that installs: it must have been seen.
    let hull = inputs(
        &hull_session_start(),
        "",
        ".claude/settings.json SessionStart",
    );
    assert!(hull.contains_key("hull/package.json"), "{hull:?}");
    assert!(
        missing.is_empty(),
        "hook inputs not trust root:\n{missing:#?}"
    );
}

fn hull_session_start() -> String {
    let v: Value = serde_json::from_str(&read(".claude/settings.json")).unwrap();
    let mut cmds = Vec::new();
    collect_commands(&v["hooks"]["SessionStart"], &mut cmds);
    cmds.into_iter()
        .find(|c| c.contains("pnpm install"))
        .expect("root SessionStart runs pnpm install")
}

fn collect_commands(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            if let Some(Value::String(c)) = m.get("command") {
                out.push(c.clone());
            }
            m.values().for_each(|x| collect_commands(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect_commands(x, out)),
        _ => {}
    }
}

#[test]
fn every_setup_script_and_what_it_reads_is_trust_root() {
    let scripts = walk(&|p| p.ends_with("docs/setup-script.sh") && !p.starts_with("site/apps/"));
    assert!(scripts.len() >= 5, "setup scripts: {scripts:?}");
    let mut missing = Vec::new();
    let mut seen = 0;
    for s in &scripts {
        if !is_trust_root(s) {
            missing.push(format!("{s} (the setup script itself)"));
        }
        // A setup script runs at the root of the checkout (the monorepo root: the cloud
        // environment's setup script is pasted from `bosun/docs/`). `cd` and `$d` in it are
        // dynamic; what it reads there is the pnpm and rust inputs, added by name.
        let mut text = read(s);
        text.push_str("\npnpm cargo\n");
        let mut found = inputs(&text, "", s);
        close(&mut found);
        for (p, why) in found {
            seen += 1;
            // Cargo files exist or not; a Cargo.toml / build.rs / lockfile is checked by path
            // only when it exists, as above, and the manifest and pnpm names always.
            if !is_trust_root(&p) {
                missing.push(format!("{p} ({why})"));
            }
        }
    }
    assert!(seen >= 5, "setup script inputs found: {seen}");
    assert!(missing.is_empty(), "not trust root:\n{missing:#?}");
}

/// Files a privileged job (secret, id-token, environment, or a base-context workflow) names.
#[test]
fn every_privileged_workflow_input_is_trust_root() {
    let files = workflows();
    assert!(files.len() >= 12, "workflows: {files:?}");
    let (mut jobs_seen, mut privileged_seen, mut inputs_seen) = (0, 0, 0);
    let mut missing = Vec::new();
    for f in &files {
        let text = read(f);
        for (id, job) in jobs(&text) {
            jobs_seen += 1;
            if !privileged(&text, &job) {
                continue;
            }
            privileged_seen += 1;
            let mut found = job_inputs(&text, &job, &format!("{f} job {id}"));
            close(&mut found);
            for (p, why) in found {
                inputs_seen += 1;
                if !is_trust_root(&p) {
                    missing.push(format!("{p} ({why})"));
                }
            }
        }
    }
    assert!(jobs_seen >= 40, "jobs: {jobs_seen}");
    assert!(privileged_seen >= 15, "privileged jobs: {privileged_seen}");
    assert!(inputs_seen >= 10, "privileged inputs: {inputs_seen}");
    assert!(missing.is_empty(), "not trust root:\n{missing:#?}");
}

/// Paths a shell text reads out of a base commit (`git show "$base:path"`, `git cat-file -e
/// "$BOSUN_BASE:path"`, `git show "${BASE}:path"`, any variable name and case). A path that
/// continues with another variable (`"$1:$p"`) is not a literal and is the `pat=` check's.
fn base_reads(text: &str) -> Vec<String> {
    let re = Regex::new(
        r#"(?i)\bgit\s+(?:-C\s+\S+\s+)?(?:show|cat-file(?:\s+-\w+)?)\s+["']?\$(?:\{[a-z_0-9]+\}|[a-z_0-9]+):([A-Za-z0-9_./-]+)"#,
    )
    .unwrap();
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

/// The `pat='...'` regexes that choose which files are restored from the base.
fn restore_pats(text: &str) -> Vec<String> {
    let re = Regex::new(r#"(?m)\bpat='([^'\n]+)'"#).unwrap();
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

/// Top-level `|` alternatives of an extended regex (not inside parentheses).
fn alternatives(pat: &str) -> Vec<String> {
    let (mut out, mut depth, mut cur) = (Vec::new(), 0, String::new());
    let mut esc = false;
    for c in pat.chars() {
        match c {
            _ if esc => {
                esc = false;
                cur.push(c);
            }
            '\\' => {
                esc = true;
                cur.push(c);
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth -= 1;
                cur.push(c);
            }
            '|' if depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// Every name a restore `pat` may select, at the root and at depth.
fn candidates() -> Vec<String> {
    let names = [
        ".cargo/config.toml",
        ".cargo/config",
        ".cargo/x/y.toml",
        "rust-toolchain",
        "rust-toolchain.toml",
        ".npmrc",
        ".pnpmfile.js",
        ".pnpmfile.cjs",
        ".pnpmfile.mjs",
        "deny.toml",
        ".deny.toml",
        "deny.exceptions.toml",
        ".deny.exceptions.toml",
        ".bosun/bosun.toml",
        ".bosun/risk.toml",
        ".bosun/approvers.toml",
        ".bosun/pins.toml",
        ".bosun/session-rules.toml",
        ".bosun/template-tests/a.toml",
        "package.json",
        "pnpm-workspace.yaml",
        "pnpm-workspace.yml",
    ];
    ["", "a/", "a/b/", "bosun/", "hull/"]
        .iter()
        .flat_map(|d| names.iter().map(move |n| format!("{d}{n}")))
        .collect()
}

#[test]
fn the_base_read_finder_sees_every_form() {
    let got = base_reads(
        r#"v="$(git show "$base:.bosun/version" 2>/dev/null)"; git show "$BASE:pins.toml"; git cat-file -e "$BOSUN_BASE:scripts/pins.mjs"; git show "${Base}:a/b.toml"; git show "$1:$p" > x; git show "$BASE:$TS/package.json"; git -C d show $sha:x.json"#,
    );
    assert_eq!(
        got,
        [
            ".bosun/version",
            "pins.toml",
            "scripts/pins.mjs",
            "a/b.toml",
            "x.json"
        ]
    );
    let pats = restore_pats("pat='(^|/)a$|^b/'\n  pat='x'");
    assert_eq!(pats, ["(^|/)a$|^b/", "x"]);
    assert_eq!(
        alternatives("(^|/)a(/|$)|^b$|c"),
        ["(^|/)a(/|$)", "^b$", "c"]
    );
}

/// The channel templates and the workflows read the base's copy of these files in the job that
/// gates the PR, or restore from the base every file a `pat=` regex selects: all of them are
/// trust root (a PR's change to one is judged by the base's copy, so no quorum may count it).
#[test]
fn what_channel_templates_read_at_the_base_is_trust_root() {
    let files = walk(&|p| {
        p.starts_with("bosun/channel/")
            || (p.starts_with(".github/workflows/") && p.ends_with(".yml"))
    });
    let mut literal = std::collections::BTreeSet::new();
    let mut pats = std::collections::BTreeSet::new();
    for t in &files {
        if let Ok(text) = fs::read_to_string(root().join(t)) {
            literal.extend(base_reads(&text));
            pats.extend(restore_pats(&text));
        }
    }
    assert!(literal.contains("pins.toml"), "{literal:?}");
    assert!(literal.len() >= 2, "{literal:?}");
    assert!(pats.len() >= 2, "{pats:?}");
    // `.bosun/version` is the pin: `pin_change_trust_root` judges it (an upgrade stays quorum).
    let mut missing: Vec<String> = (literal.iter())
        .filter(|p| p.as_str() != ".bosun/version" && !is_trust_root(p))
        .cloned()
        .collect();
    let cands = candidates();
    for pat in &pats {
        // Every alternative must select something here, else a new name needs a candidate.
        for alt in alternatives(pat) {
            let re = Regex::new(&alt).unwrap_or_else(|e| panic!("{alt}: {e}"));
            assert!(
                cands.iter().any(|c| re.is_match(c)),
                "no candidate for {alt}"
            );
        }
        let re = Regex::new(pat).unwrap_or_else(|e| panic!("{pat}: {e}"));
        for c in cands.iter().filter(|c| re.is_match(c)) {
            if c != ".bosun/version" && !is_trust_root(c) {
                missing.push(c.to_string());
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "restored from the base, not trust root:\n{missing:#?}"
    );
}

/// Patterns on one line that restore a build or package cache.
fn cache_use(line: &str) -> Option<&'static str> {
    let t = line.trim();
    let t = t.strip_prefix("- ").unwrap_or(t);
    let key = |k: &str| t.strip_prefix(k).map(unquote);
    if t.contains("actions/cache") {
        Some("actions/cache")
    } else if t.contains("rust-cache") {
        Some("Swatinem/rust-cache")
    } else if t.contains("sccache") {
        Some("sccache")
    } else if key("cache:").is_some_and(|v| !v.is_empty() && v != "false") {
        Some("cache:")
    } else if key("cache-dependency-path:").is_some() {
        Some("cache-dependency-path")
    } else if key("package-manager-cache:").is_some_and(|v| v != "false") {
        Some("package-manager-cache")
    } else if key("cache-from:").is_some() {
        Some("cache-from")
    } else if key("bundler-cache:").is_some_and(|v| !v.is_empty() && v != "false") {
        Some("bundler-cache")
    } else {
        None
    }
}

/// Whether a step sets `key: value` (an action input) as written.
fn step_sets(step: &str, key: &str, value: &str) -> bool {
    let pre = format!("{key}:");
    step.lines().any(|l| {
        let t = l.trim();
        let t = t.strip_prefix("- ").unwrap_or(t);
        t.strip_prefix(&pre).is_some_and(|v| unquote(v) == value)
    })
}

/// What a step does with a cache beyond the lines [`cache_use`] sees: a hoard step (the action
/// is a cache save and restore: `./hoard`, `./keelstack/hoard`, `*/hoard@v1`,
/// `*/hoard/action@v1`), and an action that caches by default unless switched off.
fn step_cache(step: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = (step.lines())
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(cache_use)
        .collect();
    let Some(uses) = step_key(step, "uses") else {
        return out;
    };
    let uses = uses.to_lowercase();
    let name = uses.split('@').next().unwrap_or("");
    let local = name.starts_with("./");
    if name == "./hoard"
        || (local && name.ends_with("/hoard"))
        || (!local && (name.ends_with("/hoard") || name.ends_with("/hoard/action")))
    {
        out.push("hoard (cache save and restore)");
    }
    let default_on = |off_key: &str, off: &str| !step_sets(step, off_key, off);
    let cached = match name {
        "actions/setup-go" | "actions-rust-lang/setup-rust-toolchain" | "jdx/mise-action" => {
            default_on("cache", "false")
        }
        "astral-sh/setup-uv" => default_on("enable-cache", "false"),
        "gradle/actions/setup-gradle" | "gradle/gradle-build-action" => {
            default_on("cache-disabled", "true")
        }
        _ => false,
    };
    if cached {
        out.push("an action that caches by default");
    }
    out
}

/// The one cache a secret-holding job may use: hoard's own self-test runs the cache server it
/// tests, so its `uses: ./hoard` save and restore round trip is the point. `(workflow, job, step
/// name)`; any other cache in those jobs, and every other job, still fails.
const HOARD_SELF_TEST: [(&str, &str, &str); 2] = [
    (".github/workflows/hoard-self-test.yml", "warm", "hoard"),
    (".github/workflows/hoard-self-test.yml", "hit", "hoard"),
];

/// `(kind, step name, line)` of every cache a job restores or saves.
fn cache_hits(job: &str) -> Vec<(String, String, String)> {
    let ver = Regex::new(r"actions/setup-node@\S+\s+#\s*v(\d+)").unwrap();
    let (outside, steps) = steps_of(job);
    let mut out: Vec<(String, String, String)> = code_lines(&outside)
        .into_iter()
        .filter_map(|l| cache_use(l).map(|k| (k.to_string(), String::new(), l.trim().to_string())))
        .collect();
    for step in steps {
        let name = step_key(&step, "name").unwrap_or_default();
        let uses = step_key(&step, "uses").unwrap_or_default();
        for k in step_cache(&step) {
            out.push((k.to_string(), name.clone(), uses.clone()));
        }
        // setup-node v5 and later caches a package manager on its own unless told not to.
        let major = ver.captures(&step).map(|c| c[1].parse::<u32>().unwrap());
        if step.contains("actions/setup-node")
            && (major.is_none() || major >= Some(5))
            && !step.contains("package-manager-cache: false")
        {
            let k = "setup-node without package-manager-cache: false";
            out.push((k.to_string(), name, uses));
        }
    }
    out
}

#[test]
fn jobs_holding_a_secret_or_an_id_token_restore_no_cache() {
    let (mut held, mut hits) = (Vec::new(), Vec::new());
    let mut exempt_used = 0;
    for f in workflows() {
        let text = read(&f);
        for (id, job) in jobs(&text) {
            if !holds_secret(&job) {
                continue;
            }
            held.push(format!("{f}:{id}"));
            for (kind, step, what) in cache_hits(&job) {
                let exempt = kind.starts_with("hoard")
                    && HOARD_SELF_TEST.contains(&(f.as_str(), id.as_str(), step.as_str()));
                if exempt {
                    exempt_used += 1;
                } else {
                    hits.push(format!("{f}:{id}: {kind}: step {step:?}: {what}"));
                }
            }
        }
    }
    for must in [
        ".github/workflows/release.yml",
        ".github/workflows/hoard-self-test.yml",
        ".github/workflows/deploy-site.yml",
        ".github/workflows/cargo-deny-npm.yml",
    ] {
        assert!(held.iter().any(|h| h.starts_with(must)), "{must}: {held:?}");
    }
    assert!(held.len() >= 6, "secret-holding jobs: {held:?}");
    // The exemption is used exactly where it is written: a stale one fails.
    assert_eq!(exempt_used, HOARD_SELF_TEST.len(), "hoard exemption");
    assert!(
        hits.is_empty(),
        "cache in a job holding a secret:\n{hits:#?}"
    );
}

#[test]
fn the_cache_detector_sees_every_form() {
    for l in [
        "      - uses: actions/cache@abc",
        "      - uses: actions/cache/restore@abc",
        "      - uses: Swatinem/rust-cache@abc",
        "          cache: pnpm",
        "          cache: 'pip'",
        "          cache-dependency-path: a/b",
        "          package-manager-cache: true",
        "          bundler-cache: true",
    ] {
        assert!(cache_use(l).is_some(), "{l}");
    }
    for l in [
        "          package-manager-cache: false",
        "          cache: false",
        "          cache: \"false\"",
        "        run: cargo build",
    ] {
        assert!(cache_use(l).is_none(), "{l}");
    }
    let step = |uses: &str, with: &str| format!("      - uses: {uses}\n        with:\n{with}");
    for (uses, with) in [
        ("./hoard", ""),
        ("./keelstack/hoard", ""),
        ("keelstackhq/keelstack/hoard@v1", ""),
        ("keelstackhq/keelstack/hoard/action@v1", ""),
        ("keelstackhq/hoard@abc # v1", ""),
        ("actions/setup-go@abc # v5", "          go-version: 1.23\n"),
        ("actions-rust-lang/setup-rust-toolchain@abc", ""),
        (
            "actions-rust-lang/setup-rust-toolchain@abc",
            "          cache: true\n",
        ),
        ("actions/setup-python@abc", "          cache: pip\n"),
        ("astral-sh/setup-uv@abc", ""),
        ("jdx/mise-action@abc", ""),
        ("gradle/actions/setup-gradle@abc", ""),
    ] {
        assert!(!step_cache(&step(uses, with)).is_empty(), "{uses}");
    }
    for (uses, with) in [
        ("actions/setup-go@abc", "          cache: false\n"),
        (
            "actions-rust-lang/setup-rust-toolchain@abc",
            "          cache: 'false'\n",
        ),
        ("astral-sh/setup-uv@abc", "          enable-cache: false\n"),
        (
            "gradle/actions/setup-gradle@abc",
            "          cache-disabled: true\n",
        ),
        ("./splice/action", ""),
        ("actions/checkout@abc", ""),
        ("dtolnay/rust-toolchain@abc", ""),
        ("keelstackhq/keelstack/hoist@v1", ""),
    ] {
        assert!(step_cache(&step(uses, with)).is_empty(), "{uses}");
    }
    // A `with:` input named like the key is not the step's own `uses`.
    let nested = "      - run: x\n        env:\n          uses: ./hoard\n";
    assert!(step_cache(nested).is_empty());
}

/// What makes a job privileged: a write token (job or workflow `permissions:`), a bare
/// `secrets` expression, `secrets: inherit`, a named secret, an id-token, an environment, a
/// base-context trigger.
#[test]
fn privileged_counts_write_tokens_and_whole_secrets() {
    let wf = |top: &str, job: &str| format!("name: t\non: pull_request\n{top}jobs:\n  j:\n{job}");
    let steps = "    steps:\n      - run: echo hi\n";
    let check = |top: &str, job: &str| {
        let file = wf(top, &format!("{job}{steps}"));
        let (_, j) = jobs(&file).remove(0);
        privileged(&file, &j)
    };
    let read = "permissions:\n  contents: read\n";
    assert!(!check(read, ""));
    assert!(!check(read, "    permissions: {}\n"));
    assert!(!check(read, "    permissions:\n      contents: read\n"));
    assert!(!check("permissions: read-all\n", ""));
    // Any write, at the job, or inherited from the workflow.
    assert!(check(
        read,
        "    permissions:\n      contents: read\n      issues: write\n"
    ));
    assert!(check(read, "    permissions: write-all\n"));
    assert!(check(read, "    permissions: { pull-requests: write }\n"));
    assert!(check("permissions:\n  contents: write\n", ""));
    // A job's own block replaces the workflow's.
    assert!(!check(
        "permissions:\n  contents: write\n",
        "    permissions:\n      contents: read\n"
    ));
    // A comment that says write is no grant.
    assert!(!check(
        "permissions:\n  contents: read # no write here\n",
        ""
    ));
    // No `permissions:` anywhere: the repository default is unknown, so privileged.
    assert!(check("", ""));
    // Secrets: named, indexed, bare, toJSON, inherit; the workflow's own token is not one.
    for l in [
        "      - run: echo ${{ secrets.NPM_TOKEN }}",
        "      - run: echo ${{ secrets['NPM_TOKEN'] }}",
        "      - run: echo ${{ toJSON(secrets) }}",
        "      - run: echo '${{ secrets }}'",
        "      - run: echo ${{ format('{0}', secrets) }}",
        "    secrets: inherit",
    ] {
        assert!(check(read, &format!("{l}\n")), "{l}");
    }
    assert!(!check(
        read,
        "      - run: echo ${{ secrets.GITHUB_TOKEN }}\n"
    ));
    assert!(!check(read, "      # ${{ secrets.NPM_TOKEN }}\n"));
}
