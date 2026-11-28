use super::repo;
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::exec::{Cmd, Out, Runner};
use crate::pins::{Pins, platform_key};
use crate::report::HumanStep;
use crate::step::{Check, Step};
use crate::tools::{gh, git, not_found, npm};
use serde::Serialize;
use std::path::Path;

pub struct Preflight;

/// The tools keel calls, with the binary to ask for `--version`.
const TOOLS: [&str; 4] = ["splice", "bosun", "hoist", "berth"];

fn at_pin(out: &Out, version: &str) -> bool {
    // Exact match of the last token of the first line (`splice 0.6.0`, `v0.6.0`), not a substring.
    let tok = out
        .stdout
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().last())
        .unwrap_or("");
    out.code == 0 && tok.strip_prefix('v').unwrap_or(tok) == version
}

impl Preflight {
    /// Tools not at their pin, as `(name, found)`.
    fn behind(cx: &Ctx) -> Vec<(&'static str, String)> {
        Preflight::behind_of(cx.runner, &cx.pins)
    }

    /// Same as `behind`, without a run context (the read-only `keel preflight` command).
    pub fn behind_of(runner: &dyn Runner, pins: &Pins) -> Vec<(&'static str, String)> {
        TOOLS
            .iter()
            .filter_map(|t| {
                let pin = pins.tool(t)?;
                let o = runner.run(&Cmd::new(t, &["--version"]));
                (!at_pin(&o, &pin.version)).then(|| {
                    let found = if o.code == 0 {
                        o.stdout.trim().to_string()
                    } else {
                        "none".into()
                    };
                    (*t, found)
                })
            })
            .collect()
    }
}

impl Step for Preflight {
    fn id(&self) -> &'static str {
        "preflight"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let repo = repo(cx)?;
        let r = cx.runner;

        let node = r.run(&Cmd::new("node", &["--version"]));
        if not_found(&node) {
            return Ok(Check::Failed(
                "node not found; install Node 24 or newer".into(),
            ));
        }
        let node_v = node.stdout.trim().trim_start_matches('v').to_string();
        let major: u32 = node_v
            .split('.')
            .next()
            .and_then(|m| m.parse().ok())
            .unwrap_or(0);
        if node.code != 0 || major < 24 {
            return Ok(Check::Failed(format!(
                "node {node_v} is too old; install Node 24 or newer (https://nodejs.org)"
            )));
        }

        match gh::auth_ok(r) {
            None => {
                return Ok(Check::Failed(
                    "gh not found; install the GitHub CLI (https://cli.github.com)".into(),
                ));
            }
            Some(false) => {
                return Ok(Check::Human(HumanStep {
                    id: self.id().into(),
                    title: "Log in to GitHub with the gh CLI".into(),
                    url: String::new(),
                    command: "gh auth login".into(),
                }));
            }
            Some(true) => {}
        }
        let Some(login) = gh::login(r) else {
            return Ok(Check::Failed(
                "`gh api user` failed; run `gh auth status` and `gh auth login`".into(),
            ));
        };
        let Some(git_v) = git::version(r) else {
            return Ok(Check::Failed("git not found; install git".into()));
        };
        if let Err(e) = gh::default_branch(r, &repo) {
            // keel configures an existing repo; the org provisioner app creates it (D-014).
            return Ok(Check::Human(HumanStep {
                id: self.id().into(),
                title: format!(
                    "Request {repo} from the org provisioner (keelstackhq/provision), or check the name and your access ({e})"
                ),
                url: "https://github.com/keelstackhq/provision".into(),
                command: "keel up".into(),
            }));
        }
        if !git::is_work_tree(r, &cx.dir) {
            return Ok(Check::Failed(format!(
                "{} is not a git checkout; run `git clone` of {repo} and use -C",
                cx.dir.display()
            )));
        }
        match git::origin(r, &cx.dir) {
            Some(u) if git::origin_matches(&u, &repo) => {}
            Some(u) => {
                return Ok(Check::Failed(format!(
                    "origin is {u}, not {repo}; fix it with `git remote set-url origin` or pass -C of the right checkout"
                )));
            }
            None => {
                return Ok(Check::Failed(format!(
                    "the checkout has no `origin`; run `git remote add origin https://github.com/{repo}.git`"
                )));
            }
        }
        let behind = Preflight::behind(cx);
        if !behind.is_empty() {
            let list: Vec<String> = behind
                .iter()
                .map(|(t, f)| {
                    let v = cx.pins.tool(t).map(|p| p.version).unwrap_or_default();
                    format!("{t} {v} (found {f})")
                })
                .collect();
            return Ok(Check::Todo(format!("install {}", list.join(", "))));
        }
        Ok(Check::Done(format!(
            "node {node_v}, gh as {login}, git {git_v}"
        )))
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let key = platform_key();
        let mut manual: Option<HumanStep> = None;
        for (tool, _) in Preflight::behind(cx) {
            let Some(pin) = cx.pins.tool(tool) else {
                continue;
            };
            // Pins exist for the version but not for this platform: keel cannot verify an
            // install, so a human installs it (a version with no sha512 at all stays Failed).
            if pin
                .sha512
                .as_ref()
                .is_some_and(|s| !s.platform.contains_key(&key))
            {
                let v = &pin.version;
                manual.get_or_insert(HumanStep {
                    id: self.id().into(),
                    title: format!(
                        "Install {tool} {v} by hand: keel has no pinned checksum for {key} and could not verify it"
                    ),
                    url: String::new(),
                    command: format!("npm install -g @keelstackhq/{tool}@{v}"),
                });
                continue;
            }
            if let Err(e) = npm::install_pinned(cx.runner, tool, &pin) {
                return Ok(Check::Failed(e));
            }
        }
        if let Some(h) = manual {
            return Ok(Check::Human(h));
        }
        self.check(cx)
    }
}

/// `preflight` for `template = "none"`: an existing repo. gh logged in with admin on it, git
/// checkout whose origin is the repo, `hoist` runs. No pin install in this mode.
pub struct ExistingPreflight;

impl Step for ExistingPreflight {
    fn id(&self) -> &'static str {
        "preflight"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let repo = repo(cx)?;
        let r = cx.runner;
        let human = |title: String, url: &str, command: &str| {
            Ok(Check::Human(HumanStep {
                id: "preflight".into(),
                title,
                url: url.into(),
                command: command.into(),
            }))
        };
        match gh::auth_ok(r) {
            None => {
                return Ok(Check::Failed(
                    "gh not found; install the GitHub CLI (https://cli.github.com)".into(),
                ));
            }
            Some(false) => {
                return human(
                    "Log in to GitHub with the gh CLI".into(),
                    "",
                    "gh auth login",
                );
            }
            Some(true) => {}
        }
        let Some(login) = gh::login(r) else {
            return Ok(Check::Failed(
                "`gh api user` failed; run `gh auth status` and `gh auth login`".into(),
            ));
        };
        match gh::is_admin(r, &repo) {
            Ok(true) => {}
            Ok(false) => {
                return human(
                    format!("{login} needs admin on {repo}"),
                    &format!("https://github.com/{repo}/settings/access"),
                    "gh auth login",
                );
            }
            Err(e) => return Ok(Check::Failed(e)),
        }
        let Some(git_v) = git::version(r) else {
            return Ok(Check::Failed("git not found; install git".into()));
        };
        if !git::is_work_tree(r, &cx.dir) {
            return Ok(Check::Failed(format!(
                "{} is not a git checkout; run `git clone` of {repo} and use -C",
                cx.dir.display()
            )));
        }
        match git::origin(r, &cx.dir) {
            Some(u) if git::origin_matches(&u, &repo) => {}
            Some(u) => {
                return Ok(Check::Failed(format!(
                    "origin is {u}, not {repo}; fix it with `git remote set-url origin` or pass -C of the right checkout"
                )));
            }
            None => {
                return Ok(Check::Failed(format!(
                    "the checkout has no `origin`; run `git remote add origin https://github.com/{repo}.git`"
                )));
            }
        }
        let h = r.run(&Cmd::new("hoist", &["--version"]));
        if h.code != 0 {
            return human(
                "Install hoist: build it from this checkout and put target/debug on PATH, or install it from npm".into(),
                "",
                "cargo build -p keel-hoist --locked && export PATH=$PWD/target/debug:$PATH  (or: npm install -g @keelstackhq/hoist)",
            );
        }
        Ok(Check::Done(format!(
            "gh as {login} (admin on {repo}), git {git_v}, {}",
            h.stdout.trim()
        )))
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        self.check(cx)
    }
}

// ---------------------------------------------------------------------------------------------
// `keel preflight`: read-only findings, also run first by `up`, `plan` and `migrate`.
// ---------------------------------------------------------------------------------------------

/// One thing preflight found. `severity` is `block` (stops with exit 5) or `warn`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub id: String,
    pub severity: &'static str,
    pub message: String,
    pub fix: String,
    /// Where the fix is done: `computer` or `phone`.
    pub device: &'static str,
}

fn finding(id: &str, severity: &'static str, message: String, fix: &str) -> Finding {
    Finding {
        id: id.into(),
        severity,
        message,
        fix: fix.into(),
        device: "computer",
    }
}

/// Which command runs preflight. `Full` is `keel preflight` itself (also tools and drift); the
/// others only gate: what the command checks or fixes by itself is left to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Full,
    Up,
    Plan,
    Migrate,
}

/// What preflight reads about the world; every field is injectable for tests.
pub struct Inputs<'a> {
    pub dir: &'a Path,
    pub repo: Option<&'a str>,
    pub gate: Gate,
    /// Running in a cloud agent session (`CLAUDE_CODE_REMOTE=true`).
    pub cloud: bool,
    pub pins: Option<&'a Pins>,
    /// `[ci] provider = "none"` in keel.toml: no GitHub-side gates, so a private Free-plan repo
    /// is a warning, not a block.
    pub ci_none: bool,
}

/// `OWNER/NAME` of a GitHub remote URL (https, ssh or a proxy path), if it looks like one.
pub fn repo_of_url(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches('/');
    let u = u.strip_suffix(".git").unwrap_or(u);
    let mut parts = u.rsplit(['/', ':']);
    let (name, owner) = (parts.next()?, parts.next()?);
    let ok = |s: &str| {
        !s.is_empty()
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (ok(name) && ok(owner)).then(|| format!("{owner}/{name}"))
}

/// Sources written as `gh:keelstackhq/<tool>...` instead of the monorepo form
/// `gh:keelstackhq/keelstack//<dir>`, per file.
fn legacy_sources(dir: &Path) -> Vec<(String, String)> {
    let mut files = vec![dir.join("keel.toml"), dir.join(".splice/lock.toml")];
    if let Ok(rd) = std::fs::read_dir(dir.join(".splice/channels")) {
        let mut ch: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        ch.sort();
        files.extend(ch);
    }
    let mut found = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let hit = text.split("gh:keelstackhq/").skip(1).find_map(|rest| {
            let tool: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                .collect();
            (!tool.is_empty() && tool != "keelstack").then_some(tool)
        });
        if let Some(tool) = hit {
            let rel = f.strip_prefix(dir).unwrap_or(&f).display().to_string();
            found.push((rel, tool));
        }
    }
    found
}

/// All findings, read only. Never prints or returns a token.
pub fn findings(r: &dyn Runner, i: &Inputs) -> Vec<Finding> {
    let full = i.gate == Gate::Full;
    let mut out = Vec::new();

    match gh::auth_ok(r) {
        None if full => out.push(finding(
            "gh-missing",
            "block",
            "gh (the GitHub CLI) is not installed".into(),
            "install gh (https://cli.github.com), then `gh auth login`",
        )),
        Some(false) if full => out.push(finding(
            "gh-auth",
            "block",
            "gh is not logged in".into(),
            "run `gh auth login`",
        )),
        Some(true) => {
            if let Some(scopes) = gh::token_scopes(r) {
                let has = |s: &str| scopes.iter().any(|x| x == s);
                if !has("repo") {
                    out.push(finding(
                        "scope-repo",
                        "block",
                        "the gh token has no `repo` scope".into(),
                        "run `gh auth refresh -s repo -s workflow`",
                    ));
                }
                if !has("workflow") {
                    out.push(finding(
                        "scope-workflow",
                        "warn",
                        "the gh token has no `workflow` scope: pushes that change .github/workflows are refused".into(),
                        "run `gh auth refresh -s workflow`",
                    ));
                }
            }
            if let Some(repo) = i.repo {
                repo_findings(r, repo, i.gate, i.ci_none, &mut out);
            }
        }
        _ => {}
    }

    for (file, tool) in legacy_sources(i.dir) {
        out.push(finding(
            "legacy-source",
            "warn",
            format!(
                "{file} uses the legacy source gh:keelstackhq/{tool}; the monorepo form is gh:keelstackhq/keelstack//<dir>"
            ),
            "run `keel migrate` to move to the monorepo sources",
        ));
    }

    if full {
        if let Some(pins) = i.pins {
            for (tool, found) in Preflight::behind_of(r, pins) {
                let v = pins.tool(tool).map(|p| p.version).unwrap_or_default();
                out.push(finding(
                    "tool-behind",
                    "warn",
                    format!("{tool} is not at its pin {v} (found {found})"),
                    "run `keel up` (installs the pinned tools) or `keel migrate`",
                ));
            }
        }
        if i.dir.join(".splice/lock.toml").is_file() {
            let c = Cmd::new("splice", &["--json", "check"]).dir(i.dir);
            let o = r.run(&c);
            if o.code != 0 && !not_found(&o) {
                out.push(finding(
                    "drift",
                    "warn",
                    "splice check reports drift from the template".into(),
                    "run `keel migrate` (or `splice update`) and review the diff",
                ));
            }
        }
    }

    if i.cloud {
        out.push(finding(
            "cloud-session",
            "warn",
            "cloud agent session: the proxy refuses some GitHub reads (App slugs, installations) and pushes outside the assigned branch".into(),
            "read the results marked unknown as unchecked; push only to the branch the session was given",
        ));
    }
    out
}

/// Plan, visibility and admin rights of `repo`.
fn repo_findings(r: &dyn Runner, repo: &str, gate: Gate, ci_none: bool, out: &mut Vec<Finding>) {
    let Some(info) = gh::repo_info(r, repo) else {
        if gate == Gate::Full {
            out.push(finding(
                "repo-unreadable",
                "warn",
                format!("cannot read {repo} with `gh api`: plan, visibility and admin rights are unchecked"),
                "check the name and your access: `gh repo view`",
            ));
        }
        return;
    };
    if info.admin == Some(false) {
        let sev = if matches!(gate, Gate::Full | Gate::Up) {
            "block"
        } else {
            "warn"
        };
        out.push(finding(
            "no-admin",
            sev,
            format!("the logged-in user has no admin rights on {repo}: settings, rulesets and environments cannot be changed"),
            "ask an owner for admin on the repo, or log in as one with `gh auth login`",
        ));
    }
    if info.private == Some(true) {
        let org = info.owner.as_deref().filter(|_| info.owner_is_org);
        match gh::plan_name(r, org).as_deref() {
            Some("free") if ci_none => out.push(finding(
                "plan-free-private",
                "warn",
                format!("{repo} is private on a GitHub Free plan: no rulesets, no auto-merge and no environment secrets; `[ci] provider = \"none\"` in keel.toml accepts running without GitHub-side gates"),
                "upgrade the plan or make the repo public to get the gates; remove `[ci] provider = \"none\"` then",
            )),
            Some("free") => out.push(finding(
                "plan-free-private",
                "block",
                format!("{repo} is private on a GitHub Free plan: no rulesets, no auto-merge and no environment secrets"),
                "choose one: upgrade the plan; make the repo public; or set `[ci] provider = \"none\"` and run without GitHub-side gates",
            )),
            Some(_) => {}
            None => out.push(finding(
                "plan-unknown",
                "warn",
                format!("the plan of {repo}'s owner is not readable: rulesets, auto-merge and environment secrets may be unavailable on a private repo"),
                "check https://github.com/settings/billing/plans",
            )),
        }
    }
}

/// The result of preflight, as printed.
#[derive(Debug, Clone, Serialize)]
pub struct PreflightReport {
    pub command: String,
    pub ok: bool,
    pub exit_code: i32,
    /// `ok`, `warn` or `block`.
    pub status: &'static str,
    pub findings: Vec<Finding>,
    /// The choices of every block, one line each (what to pick from).
    pub choices: Vec<String>,
}

impl PreflightReport {
    pub fn new(command: &str, findings: Vec<Finding>) -> PreflightReport {
        let blocked = findings.iter().any(|f| f.severity == "block");
        let status = if blocked {
            "block"
        } else if findings.is_empty() {
            "ok"
        } else {
            "warn"
        };
        let choices = findings
            .iter()
            .filter(|f| f.severity == "block")
            .map(|f| format!("{}: {}", f.id, f.fix))
            .collect();
        PreflightReport {
            command: command.into(),
            ok: !blocked,
            exit_code: if blocked { 5 } else { 0 },
            status,
            findings,
            choices,
        }
    }

    pub fn blocked(&self) -> bool {
        !self.ok
    }
}

#[derive(Serialize)]
struct JsonView<'a> {
    version: u32,
    #[serde(flatten)]
    report: &'a PreflightReport,
}

pub fn render_json(r: &PreflightReport) -> String {
    serde_json::to_string(&JsonView {
        version: 1,
        report: r,
    })
    .unwrap_or_default()
}

pub fn render_text(r: &PreflightReport) -> String {
    let mut s = String::new();
    for f in &r.findings {
        s.push_str(&format!(
            "{} {}: {}\n  fix: {} [{}]\n",
            f.severity, f.id, f.message, f.fix, f.device
        ));
    }
    match r.status {
        "ok" => s.push_str("preflight: ok\n"),
        "warn" => s.push_str("preflight: ok, with warnings\n"),
        _ => s.push_str(&format!(
            "preflight: blocked, nothing was written; {} stopped before it changed anything\n",
            r.command
        )),
    }
    s
}

/// Run preflight for `keel preflight` (always reports) and as the first thing of `up`, `plan` and
/// `migrate` (reports only on a block). `None`: carry on with the command.
pub fn gate(r: &dyn Runner, cli: &crate::cli::Cli) -> Option<PreflightReport> {
    use crate::cli::Command;
    use crate::manifest::Manifest;
    let (gate, flag_repo) = match &cli.command {
        Command::Preflight => (Gate::Full, None),
        Command::Up(a) => (Gate::Up, a.repo.clone()),
        Command::Plan(a) => (Gate::Plan, a.repo.clone()),
        Command::Migrate(_) => (Gate::Migrate, None),
        Command::Doctor | Command::Owner(_) => return None,
    };
    let manifest = Manifest::load(&cli.dir).ok();
    let ci_none = manifest.as_ref().is_some_and(|m| m.ci_none());
    let repo = flag_repo
        .or_else(|| manifest.as_ref().and_then(|m| m.repo.clone()))
        .or_else(|| git::origin(r, &cli.dir).and_then(|u| repo_of_url(&u)));
    let pins = (gate == Gate::Full)
        .then(|| Pins::load(&cli.dir).ok())
        .flatten();
    let cloud = std::env::var("CLAUDE_CODE_REMOTE").is_ok_and(|v| v == "true" || v == "1");
    let list = findings(
        r,
        &Inputs {
            dir: &cli.dir,
            repo: repo.as_deref(),
            gate,
            cloud,
            pins: pins.as_ref(),
            ci_none,
        },
    );
    let report = PreflightReport::new(cli.command.name(), list);
    (gate == Gate::Full || report.blocked()).then_some(report)
}

#[cfg(test)]
mod tests {
    #[test]
    fn at_pin_is_an_exact_version_match() {
        let ok = |s: &str| Out::ok(s);
        assert!(at_pin(&ok("splice 0.6.0\n"), "0.6.0"));
        assert!(at_pin(&ok("v0.6.0"), "0.6.0"));
        assert!(at_pin(&ok("0.6.0\nextra 1.0"), "0.6.0"));
        assert!(!at_pin(&ok("berth 0.2.10"), "0.2.1"));
        assert!(!at_pin(&ok("tool 10.6.0"), "0.6.0"));
        assert!(!at_pin(&ok("tool 0.6.0-rc1"), "0.6.0"));
        assert!(!at_pin(&Out::with(1, "0.6.0", ""), "0.6.0"));
    }

    use super::super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::FakeRunner;
    use std::path::PathBuf;

    fn healthy() -> FakeRunner {
        FakeRunner::new()
            .on(&["node", "--version"], Out::ok("v24.1.0\n"))
            .on(&["gh", "auth", "status"], Out::ok(""))
            .on(&["gh", "api", "user"], Out::ok("octo\n"))
            .on(&["git", "--version"], Out::ok("git version 2.47.0\n"))
            .on(
                &["gh", "repo", "view"],
                Out::ok(r#"{"defaultBranchRef":{"name":"main"}}"#),
            )
            .on(
                &["git", "rev-parse", "--is-inside-work-tree"],
                Out::ok("true\n"),
            )
            .on(
                &["git", "remote", "get-url", "origin"],
                Out::ok("https://github.com/owner/name.git\n"),
            )
            .on(&["splice", "--version"], Out::ok("splice 0.6.0\n"))
            .on(&["bosun", "--version"], Out::ok("bosun 0.21.0\n"))
            .on(&["hoist", "--version"], Out::ok("hoist 0.5.1\n"))
            .on(&["berth", "--version"], Out::ok("berth 0.2.1\n"))
    }

    fn run_check(r: &FakeRunner, pins: &str) -> Check {
        let mut cx = ctx(
            r,
            PathBuf::from("."),
            manifest("rust"),
            pins,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        Preflight.check(&mut cx).unwrap()
    }

    #[test]
    fn all_good_is_done() {
        assert_eq!(
            run_check(&healthy(), PINS),
            Check::Done("node 24.1.0, gh as octo, git 2.47.0".into())
        );
    }

    #[test]
    fn old_node_fails_with_fix() {
        let r = healthy().on(&["node", "--version"], Out::ok("v22.3.0\n"));
        let Check::Failed(m) = run_check(&r, PINS) else {
            panic!()
        };
        assert!(m.contains("22.3.0") && m.contains("Node 24"), "{m}");
    }

    #[test]
    fn gh_not_logged_in_is_human() {
        let r = healthy().on(&["gh", "auth", "status"], Out::with(1, "", "not logged in"));
        let Check::Human(h) = run_check(&r, PINS) else {
            panic!()
        };
        assert_eq!(h.command, "gh auth login");
    }

    #[test]
    fn missing_repo_is_a_human_step_to_ask_the_provisioner_never_gh_repo_create() {
        let r = healthy().on(&["gh", "repo", "view"], Out::with(1, "", "Not Found"));
        let Check::Human(h) = run_check(&r, PINS) else {
            panic!()
        };
        assert!(h.title.contains("provision"), "{}", h.title);
        assert!(!h.title.contains("gh repo create") && !h.command.contains("repo create"));
        assert_eq!(h.url, "https://github.com/keelstackhq/provision");
        assert!(!r.calls().iter().any(|c| c.line().contains("repo create")));
    }

    #[test]
    fn wrong_origin_fails() {
        let r = healthy().on(
            &["git", "remote", "get-url", "origin"],
            Out::ok("https://github.com/other/thing.git\n"),
        );
        let Check::Failed(m) = run_check(&r, PINS) else {
            panic!()
        };
        assert!(m.contains("other/thing"), "{m}");
    }

    #[test]
    fn tool_behind_is_todo() {
        let r = healthy().on(&["bosun", "--version"], Out::ok("bosun 0.20.0\n"));
        let Check::Todo(m) = run_check(&r, PINS) else {
            panic!()
        };
        assert!(m.contains("bosun 0.21.0 (found bosun 0.20.0)"), "{m}");
    }

    /// Pins text for splice with real sha512s of the two fake tarballs.
    fn pins_for(launcher: &[u8], platform: &[u8]) -> String {
        format!(
            "{PINS}\n[sha512.splice]\nversion = \"0.6.0\"\nlauncher = \"{}\"\n{} = \"{}\"\n",
            npm::sha512_b64(launcher),
            platform_key(),
            npm::sha512_b64(platform)
        )
    }

    fn fixture(good: bool) -> (tempfile::TempDir, FakeRunner, String) {
        let tars = tempfile::tempdir().unwrap();
        let (l, p) = (tars.path().join("l.tgz"), tars.path().join("p.tgz"));
        std::fs::write(&l, b"launcher").unwrap();
        std::fs::write(
            &p,
            if good {
                &b"platform"[..]
            } else {
                &b"tampered"[..]
            },
        )
        .unwrap();
        let plat_pkg = format!(
            "@keelstackhq/splice-{}@0.6.0",
            npm::npm_platform(&platform_key())
        );
        let r = healthy()
            .on(&["splice", "--version"], Out::ok("splice 0.5.0\n"))
            // `npm pack` prints an absolute path here, which `dir.join` keeps as is.
            .on(
                &["npm", "pack", "--silent", "@keelstackhq/splice@0.6.0"],
                Out::ok(&format!("{}\n", l.display())),
            )
            .on(
                &["npm", "pack", "--silent", &plat_pkg],
                Out::ok(&format!("{}\n", p.display())),
            )
            .on(&["npm", "install"], Out::ok(""));
        (tars, r, pins_for(b"launcher", b"platform"))
    }

    #[test]
    fn install_with_good_sha_runs_npm_install_g() {
        let (_t, r, pins) = fixture(true);
        let mut cx = ctx(
            &r,
            PathBuf::from("."),
            manifest("rust"),
            &pins,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        let _ = Preflight.apply(&mut cx).unwrap();
        let inst = r
            .calls()
            .into_iter()
            .find(|c| c.program == "npm" && c.args[0] == "install")
            .unwrap();
        assert_eq!(
            &inst.args[..4],
            ["install", "-g", "--ignore-scripts", "--omit=optional"]
        );
        assert_eq!(inst.args.len(), 6);
        assert!(inst.args[4].ends_with("l.tgz") && inst.args[5].ends_with("p.tgz"));
    }

    #[test]
    fn bad_sha_fails_and_installs_nothing() {
        let (_t, r, pins) = fixture(false);
        let mut cx = ctx(
            &r,
            PathBuf::from("."),
            manifest("rust"),
            &pins,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        let Check::Failed(m) = Preflight.apply(&mut cx).unwrap() else {
            panic!()
        };
        assert!(m.contains("mismatch"), "{m}");
        assert!(
            !r.calls()
                .iter()
                .any(|c| c.program == "npm" && c.args[0] == "install")
        );
    }

    #[test]
    fn berth_behind_is_todo() {
        let r = healthy().on(&["berth", "--version"], Out::ok("berth 0.1.0\n"));
        let Check::Todo(m) = run_check(&r, PINS) else {
            panic!()
        };
        assert!(m.contains("berth 0.2.1 (found berth 0.1.0)"), "{m}");
    }

    #[test]
    fn platform_without_pin_is_a_human_step_and_installs_nothing() {
        let r = healthy().on(&["splice", "--version"], Out::ok("splice 0.5.0\n"));
        let pins = format!(
            "{PINS}\n[sha512.splice]\nversion = \"0.6.0\"\nlauncher = \"x\"\nplan9_mips = \"y\"\n"
        );
        let mut cx = ctx(
            &r,
            PathBuf::from("."),
            manifest("rust"),
            &pins,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        let Check::Human(h) = Preflight.apply(&mut cx).unwrap() else {
            panic!()
        };
        assert_eq!(h.id, "preflight");
        assert_eq!(h.command, "npm install -g @keelstackhq/splice@0.6.0");
        assert!(h.title.contains("could not verify"), "{}", h.title);
        assert!(!r.calls().iter().any(|c| c.program == "npm"));
    }

    #[test]
    fn no_sha512_never_installs() {
        let r = healthy().on(&["splice", "--version"], Out::ok("splice 0.5.0\n"));
        let mut cx = ctx(
            &r,
            PathBuf::from("."),
            manifest("rust"),
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        let Check::Failed(m) = Preflight.apply(&mut cx).unwrap() else {
            panic!()
        };
        assert!(m.contains("KEEL_PINS") && m.contains("update keel"), "{m}");
        assert!(!r.calls().iter().any(|c| c.program == "npm"));
    }
}
