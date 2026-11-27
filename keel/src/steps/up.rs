//! `keel up` on an existing keelstack repo (K5): the whole move in one command. Preflight has run
//! already (it gates every `up`). Then: repoint and migrate (`migrate::run`, one PR), the bosun
//! mode preset (`bosun migrate --to MODE`), `bosun gate-app advance` (PR A, later PR B) and ONE
//! owner line. Every step is a subprocess; keel never merges.
use crate::cli::{Cli, Command, MigrateArgs, UpArgs};
use crate::error::KeelError;
use crate::exec::{Cmd, Runner};
use crate::migrate::{self, MAction, MigrateReport};
use crate::report::{Action, ChecklistEntry, HumanStep, Report, Status, StepResult};
use crate::tools::{failure, json, not_found};
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

const TOOL_TIMEOUT: Duration = Duration::from_secs(600);
pub const DEFAULT_MODE: &str = "autonomous";

/// Which side of `keel up` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpPath {
    Bootstrap,
    Upgrade,
}

impl UpPath {
    pub fn as_str(self) -> &'static str {
        match self {
            UpPath::Bootstrap => "bootstrap",
            UpPath::Upgrade => "upgrade",
        }
    }
}

/// Dispatch on `.splice/lock.toml` (`--new` / `--existing` force a side) and say why.
pub fn choose(dir: &Path, a: &UpArgs) -> Result<(UpPath, String), KeelError> {
    let lock = dir.join(".splice/lock.toml").is_file();
    let (path, reason) = if a.new {
        (UpPath::Bootstrap, "--new: bootstrap forced".to_string())
    } else if a.existing {
        if !lock {
            return Err(KeelError::Fix {
                message: "--existing needs `.splice/lock.toml`: this checkout has no splice lock, so it is not an existing keelstack repo".into(),
                fix: "run `keel up --new --repo OWNER/NAME --template rust` to bootstrap, or -C the right checkout".into(),
            });
        }
        (UpPath::Upgrade, "--existing: upgrade forced".to_string())
    } else if lock && bootstrapping(dir, a) {
        (
            UpPath::Bootstrap,
            "keel.toml names a template: this repo is mid-bootstrap, continue it".to_string(),
        )
    } else if lock {
        (
            UpPath::Upgrade,
            ".splice/lock.toml found: an existing keelstack repo, upgrade".to_string(),
        )
    } else {
        (
            UpPath::Bootstrap,
            "no .splice/lock.toml: a new repo, bootstrap".to_string(),
        )
    };
    if path == UpPath::Bootstrap {
        let flag = [
            ("--to", a.to.is_some()),
            ("--mode", a.mode.is_some()),
            ("--branch", a.branch.is_some()),
            ("--dry-run", a.dry_run),
        ]
        .into_iter()
        .find_map(|(f, set)| set.then_some(f));
        if let Some(f) = flag {
            return Err(KeelError::Usage(format!(
                "{f} is for the upgrade path (a repo with `.splice/lock.toml`); this run is a bootstrap ({reason}); use `keel plan` to preview a bootstrap"
            )));
        }
    }
    Ok((path, reason))
}

/// A repo whose bootstrap `keel up` wrote `keel.toml` with a `template` (or that is given one by
/// flag) is still being bootstrapped: rerunning `keel up` continues it even though the render
/// step already wrote `.splice/lock.toml`. `--existing` forces the upgrade.
fn bootstrapping(dir: &Path, a: &UpArgs) -> bool {
    if a.template.is_some() || a.pieces.is_some() {
        return true;
    }
    crate::manifest::Manifest::load(dir)
        .ok()
        .and_then(|m| m.template)
        .is_some_and(|t| t != "none")
}

/// The pipeline's result: the report (same shape as a bootstrap report) plus what `data` says.
pub struct UpReport {
    pub report: Report,
    pub reason: String,
    pub target: String,
    pub mode: String,
    pub dry_run: bool,
    pub migrate: MigrateReport,
}

fn action_of(a: MAction) -> Action {
    match a {
        MAction::Ok | MAction::Skipped => Action::Ok,
        MAction::Done => Action::Done,
        MAction::WouldRun => Action::WouldRun,
        MAction::Pending => Action::Pending,
        MAction::Failed => Action::Failed,
        MAction::NotReached => Action::NotReached,
    }
}

fn owner_line(target: &str) -> String {
    format!("npx -y @keelstackhq/keel@{target} owner")
}

/// What the PR body says: each step after this PR, and the one owner line.
fn body_note(target: &str, mode: &str) -> String {
    let mut s = String::from(
        "## keel up\n\nOne command moved this repo; the steps, in order:\n\n1. preflight (read-only, before any write)\n2. repoint and migrate: this PR\n",
    );
    s.push_str(&format!(
        "3. mode preset: `bosun migrate --to {mode}` (opens its own gated PR)\n"
    ));
    if mode == DEFAULT_MODE {
        s.push_str(
            "4. gate App: `bosun gate-app advance` (PR A now, PR B after the owner's run)\n\n## Owner, once, on a computer\n\n",
        );
        s.push_str(&format!("`{}`\n", owner_line(target)));
        s.push_str("\nThen run `keel up` again: it makes PR B.\n");
    }
    s
}

fn step(id: &str, action: Action, detail: impl Into<String>) -> StepResult {
    StepResult {
        id: id.into(),
        action,
        detail: detail.into(),
    }
}

fn bosun(dir: &Path, args: &[&str]) -> Cmd {
    Cmd::new("bosun", args).dir(dir).timeout(TOOL_TIMEOUT)
}

/// `bosun migrate --to MODE`.
fn mode_step(r: &dyn Runner, dir: &Path, mode: &str) -> StepResult {
    let c = bosun(dir, &["--json", "migrate", "--to", mode]);
    let o = r.run(&c);
    if o.code == 0 {
        step(
            "mode-preset",
            Action::Done,
            format!("bosun migrate --to {mode}"),
        )
    } else {
        step("mode-preset", Action::Failed, failure(&c, &o))
    }
}

/// `bosun gate-app advance`: Ok(step, owner_done). An old bosun without it is `pending`.
fn gate_step(r: &dyn Runner, dir: &Path) -> (StepResult, bool) {
    let c = bosun(dir, &["--json", "gate-app", "advance"]);
    let o = r.run(&c);
    if not_found(&o) || o.code == 2 {
        return (
            step(
                "gate-app",
                Action::Pending,
                "this bosun has no `gate-app advance`; update bosun (keel up --to a newer release) and rerun",
            ),
            false,
        );
    }
    let v = json(&o);
    let data = v
        .as_ref()
        .map(|v| v.get("data").cloned().unwrap_or(v.clone()))
        .unwrap_or(Value::Null);
    let phase = data["step"].as_str().unwrap_or("advanced");
    let owner_done = matches!(phase, "pr-b" | "done");
    if o.code != 0 && o.code != 5 {
        return (step("gate-app", Action::Failed, failure(&c, &o)), false);
    }
    let detail = match data["pr"]["number"].as_u64() {
        Some(n) => format!("{phase}: PR #{n}"),
        None => phase.to_string(),
    };
    let action = if o.code == 5 {
        Action::Pending
    } else {
        Action::Done
    };
    (step("gate-app", action, detail), owner_done)
}

fn checklist_human(e: &ChecklistEntry) -> HumanStep {
    HumanStep {
        id: e.id.clone(),
        title: e.title.clone(),
        url: e.url.clone(),
        command: e.command.clone(),
    }
}

/// Run the upgrade pipeline in `cli.dir`.
pub fn run(cli: &Cli, reason: &str, r: &dyn Runner) -> Result<UpReport, KeelError> {
    let Command::Up(a) = &cli.command else {
        return Err(KeelError::Usage("not an up command".into()));
    };
    let dir = cli.dir.as_path();
    let mode = a.mode.clone().unwrap_or_else(|| DEFAULT_MODE.into());
    let target =
        a.to.clone()
            .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let margs = MigrateArgs {
        to: Some(target.clone()),
        dry_run: a.dry_run,
        // A cloud session pushes its own branch and opens no PR; otherwise ONE PR.
        pr: a.branch.is_none() && !a.dry_run,
        branch: a.branch.clone(),
        drift: "seed".into(),
        body_note: Some(body_note(&target, &mode)),
        ..MigrateArgs::default()
    };
    let m = migrate::run(dir, &margs, r)?;
    let mut steps = vec![step(
        "preflight",
        Action::Ok,
        "checked before any write (see `keel preflight`)",
    )];
    steps.extend(m.steps.iter().map(|s| StepResult {
        id: s.id.clone(),
        action: action_of(s.action),
        detail: s.detail.clone(),
    }));
    let autonomous = mode == DEFAULT_MODE;
    let mut failed = m.exit_code != 0 && m.exit_code != 5 && !a.dry_run;
    let mut owner_done = false;
    let rest: Vec<&str> = if autonomous {
        vec!["mode-preset", "gate-app"]
    } else {
        vec!["mode-preset"]
    };
    for id in rest {
        let s = if failed {
            step(id, Action::NotReached, "")
        } else if a.dry_run {
            let what = match id {
                "mode-preset" => format!("would run: bosun migrate --to {mode}"),
                _ => "would run: bosun gate-app advance".into(),
            };
            step(id, Action::WouldRun, what)
        } else if id == "mode-preset" {
            mode_step(r, dir, &mode)
        } else {
            let (s, done) = gate_step(r, dir);
            owner_done = done;
            s
        };
        failed |= s.action == Action::Failed;
        steps.push(s);
    }
    if autonomous {
        steps.push(step(
            "owner",
            if failed || a.dry_run {
                Action::NotReached
            } else if owner_done {
                Action::Ok
            } else {
                Action::Pending
            },
            if owner_done {
                "the owner's run is finished".to_string()
            } else {
                owner_line(&target)
            },
        ));
    }
    let mut checklist: Vec<ChecklistEntry> = m.checklist.clone();
    if autonomous && !failed && !a.dry_run && !owner_done {
        let h = HumanStep {
            id: "owner".into(),
            title: "Run the owner command once, on a computer: it creates the gate App and the signer, approves PR A and finishes the setup".into(),
            url: String::new(),
            command: owner_line(&target),
        };
        checklist.extend(crate::report::build_checklist(&[h], &[]));
    }
    // Computer entries first, otherwise in order.
    let (mut computer, phone): (Vec<_>, Vec<_>) =
        checklist.into_iter().partition(|e| e.device == "computer");
    computer.extend(phone);
    let checklist = computer;
    let exit_code = if failed {
        if m.exit_code != 0 && m.exit_code != 5 {
            m.exit_code
        } else {
            1
        }
    } else if a.dry_run {
        m.exit_code
    } else if checklist.is_empty() {
        0
    } else {
        5
    };
    let status = if failed {
        Status::Failed
    } else if a.dry_run {
        Status::Planned
    } else if checklist.is_empty() {
        Status::Done
    } else {
        Status::Human
    };
    let report = Report {
        command: "up".into(),
        status,
        steps,
        human_step: checklist.first().map(checklist_human),
        exit_code,
        items: Vec::new(),
        checklist,
        verify: None,
    };
    Ok(UpReport {
        report,
        reason: reason.into(),
        target,
        mode,
        dry_run: a.dry_run,
        migrate: m,
    })
}

/// The `data` object of an `up` JSON report.
pub fn data(path: UpPath, reason: &str, extra: Option<&UpReport>) -> Value {
    let mut d = json!({ "path": path.as_str(), "reason": reason });
    if let Some(u) = extra {
        d["target"] = u.target.clone().into();
        d["mode"] = u.mode.clone().into();
        d["dry_run"] = u.dry_run.into();
        d["branch"] = serde_json::to_value(&u.migrate.branch).unwrap_or(Value::Null);
        d["pr"] = serde_json::to_value(&u.migrate.pr).unwrap_or(Value::Null);
        d["migrate"] = serde_json::to_value(&u.migrate).unwrap_or(Value::Null);
    }
    d
}

/// The report as one JSON object with `data.path` and `data.reason`.
pub fn render_json(
    report: &Report,
    path: UpPath,
    reason: &str,
    extra: Option<&UpReport>,
) -> String {
    let mut v: Value = serde_json::from_str(&crate::report::render_json(report))
        .expect("report serializes to an object");
    v["data"] = data(path, reason, extra);
    v.to_string()
}

pub fn render_text(u: &UpReport) -> String {
    let mut out = format!("path: upgrade ({})\n", u.reason);
    out.push_str(&crate::report::render_text(&u.report));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(lock: bool) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        if lock {
            std::fs::create_dir(d.path().join(".splice")).unwrap();
            std::fs::write(d.path().join(".splice/lock.toml"), "").unwrap();
        }
        d
    }

    #[test]
    fn the_lock_file_picks_the_side_and_the_flags_force_it() {
        let a = UpArgs::default();
        assert_eq!(choose(dir(false).path(), &a).unwrap().0, UpPath::Bootstrap);
        assert_eq!(choose(dir(true).path(), &a).unwrap().0, UpPath::Upgrade);
        let new = UpArgs {
            new: true,
            ..UpArgs::default()
        };
        assert_eq!(choose(dir(true).path(), &new).unwrap().0, UpPath::Bootstrap);
        let ex = UpArgs {
            existing: true,
            ..UpArgs::default()
        };
        assert_eq!(choose(dir(true).path(), &ex).unwrap().0, UpPath::Upgrade);
        assert!(choose(dir(false).path(), &ex).is_err());
        // A repo mid-bootstrap (keel.toml names a template) continues its bootstrap.
        let mid = dir(true);
        std::fs::write(mid.path().join("keel.toml"), "template = \"rust\"\n").unwrap();
        assert_eq!(choose(mid.path(), &a).unwrap().0, UpPath::Bootstrap);
        assert_eq!(choose(mid.path(), &ex).unwrap().0, UpPath::Upgrade);
    }

    #[test]
    fn upgrade_only_flags_are_refused_on_the_bootstrap_path() {
        let a = UpArgs {
            dry_run: true,
            ..UpArgs::default()
        };
        let e = choose(dir(false).path(), &a).unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("--dry-run"), "{e}");
    }

    fn gate(code: i32, data: &str) -> (StepResult, bool) {
        use crate::exec::{FakeRunner, Out};
        let body = format!(r#"{{"ok":true,"data":{data}}}"#);
        let r = FakeRunner::new().on(
            &["bosun", "--json", "gate-app", "advance"],
            Out::with(code, &body, ""),
        );
        gate_step(&r, Path::new("."))
    }

    #[test]
    fn gate_step_reads_the_real_bosun_shape() {
        let (s, done) = gate(
            0,
            r#"{"dry_run":false,"app_id":1,"slug":"g","step":"pr-a","status":"advanced","steps":[],"left":[],"pr":{"number":10,"head_sha":"abc"}}"#,
        );
        assert_eq!(s.action, Action::Done);
        assert_eq!(s.detail, "pr-a: PR #10");
        assert!(!done);
        let (s, done) = gate(
            5,
            r#"{"step":"probe","status":"waiting","next":["hoist apply"]}"#,
        );
        assert_eq!(
            (s.action, s.detail.as_str(), done),
            (Action::Pending, "probe", false)
        );
        let (_, done) = gate(0, r#"{"step":"hoist-apply","pr":{"number":10}}"#);
        assert!(!done);
        let (s, done) = gate(0, r#"{"step":"pr-b","pr":{"number":11}}"#);
        assert!(done && s.detail.contains("#11"), "{s:?}");
        assert!(gate(0, r#"{"step":"done"}"#).1);
    }

    #[test]
    fn the_pr_body_names_every_step_and_the_owner_line() {
        let n = body_note("0.25.0", "autonomous");
        for s in [
            "preflight",
            "bosun migrate --to autonomous",
            "gate-app advance",
            "npx -y @keelstackhq/keel@0.25.0 owner",
        ] {
            assert!(n.contains(s), "{s}: {n}");
        }
        assert!(!body_note("0.25.0", "steady").contains("owner"));
    }

    #[test]
    fn a_bootstrap_report_gets_data_path_and_reason() {
        let r = Report {
            command: "up".into(),
            status: Status::Done,
            steps: vec![],
            human_step: None,
            exit_code: 0,
            items: vec![],
            checklist: vec![],
            verify: None,
        };
        let v: Value = serde_json::from_str(&render_json(
            &r,
            UpPath::Bootstrap,
            "no .splice/lock.toml: a new repo, bootstrap",
            None,
        ))
        .unwrap();
        assert_eq!(v["data"]["path"], "bootstrap");
        assert!(v["data"]["reason"].as_str().unwrap().contains("bootstrap"));
        assert_eq!(v["command"], "up");
    }
}
