use super::repo;
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::report::HumanStep;
use crate::step::{Check, Step};
use crate::tools::bosun::{self, Res};
use crate::tools::{gh, git, splice};

pub struct Governance;

fn human(title: String, command: String) -> Check {
    Check::Human(HumanStep {
        id: "governance".into(),
        title,
        url: String::new(),
        command,
    })
}

fn pin(cx: &Ctx) -> String {
    cx.pins.tool("bosun").map(|p| p.version).unwrap_or_default()
}

/// What `plan import` reads: the manifest `plan` file, else bosun's default (`plans/*.md`, when
/// any exist). `None`: no plan to import.
fn plan_target(cx: &Ctx) -> Option<Option<String>> {
    if let Some(p) = &cx.manifest.plan {
        return Some(Some(p.clone()));
    }
    let any = std::fs::read_dir(cx.dir.join("plans"))
        .map(|d| {
            d.flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "md"))
        })
        .unwrap_or(false);
    any.then_some(None)
}

impl Step for Governance {
    fn id(&self) -> &'static str {
        "governance"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        if !cx.dir.join(".bosun/version").is_file() {
            return Ok(Check::Todo("bosun init".into()));
        }
        let Some(target) = plan_target(cx) else {
            return Ok(Check::Done("bosun initialised (.bosun/version)".into()));
        };
        if let Some(plan) = target.as_ref().filter(|p| !cx.dir.join(p).is_file()) {
            return Ok(Check::Failed(format!(
                "plan file {plan} not found; fix `plan` in keel.toml or add the file"
            )));
        }
        let plan = target.unwrap_or_else(|| "plans/*.md".into());
        let file = cx.manifest.plan.as_deref();
        Ok(
            match bosun::plan_import(cx.runner, &cx.dir, file, false, true, &pin(cx)) {
                Res::Ok(data) => {
                    let n = bosun::would_create(&data);
                    if n == 0 {
                        Check::Done(format!("bosun initialised, plan {plan} imported"))
                    } else {
                        Check::Todo(format!("bosun plan import {plan} ({n} to create)"))
                    }
                }
                Res::Human { title, command } => human(title, command),
                Res::Failed(e) => Check::Failed(e),
            },
        )
    }

    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let repo = repo(cx)?;
        let r = cx.runner;
        let bosun_pin = pin(cx);
        let default = match gh::default_branch(r, &repo) {
            Ok(b) => b,
            Err(e) => return Ok(Check::Failed(e)),
        };
        // bosun init and plan import write to the checkout: be on the work branch first (§5.1).
        let branch = match git::ensure_branch(r, &cx.dir, &default, cx.manifest.branch_name()) {
            Ok(b) => b,
            Err(e) => return Ok(Check::Failed(e)),
        };
        let mut notes: Vec<String> = Vec::new();
        if !cx.dir.join(".bosun/version").is_file() {
            let Some(owner) = gh::login(r) else {
                return Ok(Check::Failed(
                    "`gh api user` failed; run `gh auth login`".into(),
                ));
            };
            let rust =
                cx.manifest.template.as_deref().map(splice::source) == Some(splice::RUST_SOURCE);
            let c = bosun::init_cmd(&cx.dir, &owner, &branch, rust);
            match bosun::run(r, &c, &bosun_pin) {
                Res::Ok(data) => {
                    // Create exactly the labels bosun could not (`labels_todo`), else the fallback.
                    for name in bosun::missing_labels(&data) {
                        if let Err(e) = gh::label_create(r, &repo, &name) {
                            return Ok(Check::Failed(e));
                        }
                    }
                    notes.extend(bosun::init_notes(&data));
                }
                Res::Human { title, command } => return Ok(human(title, command)),
                Res::Failed(e) => return Ok(Check::Failed(e)),
            }
        }
        if let Some(file) = plan_target(cx) {
            match bosun::plan_import(r, &cx.dir, file.as_deref(), true, false, &bosun_pin) {
                Res::Ok(data) => {
                    let n = bosun::prereqs_fixed(&data);
                    if n > 0 {
                        notes.push(format!("{n} plan prerequisite(s) fixed"));
                    }
                }
                Res::Human { title, command } => return Ok(human(title, command)),
                Res::Failed(e) => return Ok(Check::Failed(e)),
            }
        }
        Ok(match self.check(cx)? {
            Check::Done(d) if !notes.is_empty() => {
                Check::Done(format!("{d} ({})", notes.join(", ")))
            }
            c => c,
        })
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
            .on(&["gh", "api", "user"], Out::ok("octo\n"))
            .on(
                &["gh", "repo", "view"],
                Out::ok(r#"{"defaultBranchRef":{"name":"main"}}"#),
            )
            .on(
                &["git", "rev-parse", "--abbrev-ref", "HEAD"],
                Out::ok("keel/bootstrap\n"),
            )
            .on(
                &["bosun", "--json", "init"],
                Out::ok(r#"{"ok":true,"data":{"labels_created":[]}}"#),
            )
            .on(&["gh", "label", "create"], Out::ok(""))
            .on(
                &["bosun", "--json", "plan", "import"],
                Out::ok(r#"{"ok":true,"data":{"epics":[{"action":"unchanged"}],"tasks":[]}}"#),
            )
    }

    fn lines(r: &FakeRunner) -> Vec<String> {
        r.calls().iter().map(|c| c.line()).collect()
    }

    fn apply(r: &FakeRunner, dir: &std::path::Path, tpl: &str, plan: Option<&str>) -> Check {
        let mut m = manifest(tpl);
        m.plan = plan.map(str::to_string);
        let mut cx = ctx(
            r,
            dir.to_path_buf(),
            m,
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        Governance.apply(&mut cx).unwrap()
    }

    #[test]
    fn init_argv_rust_and_ts() {
        let d = tempfile::tempdir().unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "rust", None);
        assert!(lines(&r).contains(
            &"bosun --json init --owner octo --branch keel/bootstrap --no-pr --stack rust".into()
        ));
        let r = fake();
        let _ = apply(&r, d.path(), "ts", None);
        assert!(
            lines(&r)
                .contains(&"bosun --json init --owner octo --branch keel/bootstrap --no-pr".into())
        );
    }

    #[test]
    fn full_rust_source_gets_stack_rust() {
        let d = tempfile::tempdir().unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "gh:keelstackhq/hull//template-rust", None);
        assert!(
            lines(&r)
                .iter()
                .any(|l| l.starts_with("bosun --json init") && l.ends_with("--stack rust"))
        );
    }

    #[test]
    fn apply_switches_off_the_default_branch_before_bosun() {
        let d = tempfile::tempdir().unwrap();
        let r = fake()
            .on(
                &["git", "rev-parse", "--abbrev-ref", "HEAD"],
                Out::ok("main\n"),
            )
            .on(&["git", "rev-parse", "--verify"], Out::with(1, "", ""))
            .on(&["git", "checkout"], Out::ok(""));
        let _ = apply(&r, d.path(), "rust", None);
        let l = lines(&r);
        let p = |s: &str| l.iter().position(|x| x.starts_with(s)).unwrap();
        assert!(p("git checkout -b keel/bootstrap") < p("bosun --json init"));
    }

    #[test]
    fn plan_import_only_with_plan() {
        let d = tempfile::tempdir().unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "rust", None);
        assert!(!lines(&r).iter().any(|l| l.contains("plan import")));
        let r = fake();
        let _ = apply(&r, d.path(), "rust", Some("docs/plan.md"));
        assert!(lines(&r).contains(&"bosun --json plan import docs/plan.md --fix".into()));
    }

    #[test]
    fn plans_dir_is_imported_with_bosuns_default_and_fix() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("plans")).unwrap();
        std::fs::write(d.path().join("plans/a.md"), "# a").unwrap();
        std::fs::create_dir(d.path().join(".bosun")).unwrap();
        std::fs::write(d.path().join(".bosun/version"), "0.21.0").unwrap();
        let r = fake().on(
            &["bosun", "--json", "plan", "import"],
            Out::ok(r#"{"ok":true,"data":{"epics":[],"tasks":[],"prereqs_fixed":2}}"#),
        );
        let Check::Done(m) = apply(&r, d.path(), "rust", None) else {
            panic!()
        };
        assert!(lines(&r).contains(&"bosun --json plan import --fix".into()));
        assert!(m.contains("2 plan prerequisite(s) fixed"), "{m}");
        // The dry-run check uses the default too.
        let r = fake();
        let mut cx = ctx(
            &r,
            d.path().into(),
            manifest("rust"),
            PINS,
            Opts::default(),
            &no_sleep,
            &no_stdin,
        );
        assert!(matches!(Governance.check(&mut cx).unwrap(), Check::Done(_)));
        assert!(lines(&r).contains(&"bosun --json plan import --dry-run".into()));
    }

    #[test]
    fn labels_todo_creates_exactly_those_and_notes_reuse() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(
            &["bosun", "--json", "init"],
            Out::ok(
                r#"{"ok":true,"data":{"branch_reused":true,"owner":"octo","pr":null,"labels_todo":["bosun:halt"]}}"#,
            ),
        );
        let _ = apply(&r, d.path(), "rust", None);
        let l = lines(&r);
        let made: Vec<_> = l
            .iter()
            .filter(|x| x.starts_with("gh label create"))
            .collect();
        assert_eq!(
            made,
            ["gh label create bosun:halt --repo owner/name --force"]
        );
    }

    #[test]
    fn labels_fallback_only_for_what_init_did_not_report() {
        let d = tempfile::tempdir().unwrap();
        let r = fake();
        let _ = apply(&r, d.path(), "rust", None);
        let l = lines(&r);
        assert_eq!(
            l.iter()
                .filter(|x| x.starts_with("gh label create"))
                .count(),
            12
        );
        assert!(l.contains(&"gh label create bosun:halt --repo owner/name --force".into()));
        // bosun reports every label (`labels_created`): none is created here.
        let all = serde_json::to_string(&bosun::LABELS).unwrap();
        let r = fake().on(
            &["bosun", "--json", "init"],
            Out::ok(&format!(
                r#"{{"ok":true,"data":{{"labels_created":{all}}}}}"#
            )),
        );
        let _ = apply(&r, d.path(), "rust", None);
        assert!(!lines(&r).iter().any(|x| x.starts_with("gh label")));
        // `labels` works too; a partial list leaves the rest to the fallback.
        let r = fake().on(
            &["bosun", "--json", "init"],
            Out::ok(r#"{"ok":true,"data":{"labels":["bosun:halt"]}}"#),
        );
        let _ = apply(&r, d.path(), "rust", None);
        let l = lines(&r);
        assert_eq!(
            l.iter()
                .filter(|x| x.starts_with("gh label create"))
                .count(),
            11
        );
        assert!(!l.contains(&"gh label create bosun:halt --repo owner/name --force".into()));
    }

    #[test]
    fn exit_5_is_human_from_error_fix() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(
            &["bosun", "--json", "init"],
            Out::with(5, r#"{"ok":false,"exit_code":5,"error":{"kind":"x","message":"m","fix":"Add yourself to owners.toml"}}"#, ""),
        );
        let Check::Human(h) = apply(&r, d.path(), "rust", None) else {
            panic!()
        };
        assert_eq!(h.title, "Add yourself to owners.toml");
        assert_eq!(h.id, "governance");
    }

    #[test]
    fn init_on_an_existing_branch_is_a_human_step_to_upgrade_bosun() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(
            &["bosun", "--json", "init"],
            Out::with(
                3,
                r#"{"ok":false,"exit_code":3,"error":{"kind":"conflict","message":"branch keel/bootstrap already exists"}}"#,
                "",
            ),
        );
        let Check::Human(h) = apply(&r, d.path(), "rust", None) else {
            panic!()
        };
        assert!(
            h.title
                .starts_with("bosun 0.21.0 cannot init on an existing branch")
        );
        assert_eq!(h.command, "npm install -g @keelstackhq/bosun@latest");
    }

    #[test]
    fn exit_2_is_an_upgrade_human_step_not_a_crash() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(
            &["bosun", "--json", "init"],
            Out::with(2, "", "unexpected argument"),
        );
        let Check::Human(h) = apply(&r, d.path(), "rust", None) else {
            panic!()
        };
        assert_eq!(h.command, "npm install -g @keelstackhq/bosun@0.21.0");
        assert!(h.title.contains("0.21.0"));
    }

    #[test]
    fn non_json_output_is_failed_naming_the_command() {
        let d = tempfile::tempdir().unwrap();
        let r = fake().on(&["bosun", "--json", "init"], Out::ok("hello"));
        let Check::Failed(m) = apply(&r, d.path(), "rust", None) else {
            panic!()
        };
        assert!(m.contains("bosun --json init"), "{m}");
    }

    #[test]
    fn check_states() {
        let d = tempfile::tempdir().unwrap();
        let run = |r: &FakeRunner, plan: Option<&str>| {
            let mut m = manifest("rust");
            m.plan = plan.map(str::to_string);
            let mut cx = ctx(
                r,
                d.path().into(),
                m,
                PINS,
                Opts::default(),
                &no_sleep,
                &no_stdin,
            );
            Governance.check(&mut cx).unwrap()
        };
        let r = fake();
        assert!(matches!(run(&r, None), Check::Todo(_)));
        std::fs::create_dir(d.path().join(".bosun")).unwrap();
        std::fs::write(d.path().join(".bosun/version"), "0.21.0").unwrap();
        assert!(matches!(run(&r, None), Check::Done(_)));
        assert!(matches!(run(&r, Some("plan.md")), Check::Failed(_)));
        std::fs::write(d.path().join("plan.md"), "# plan").unwrap();
        assert!(matches!(run(&r, Some("plan.md")), Check::Done(_)));
        assert!(lines(&r).contains(&"bosun --json plan import plan.md --dry-run".into()));
        let r = fake().on(
            &["bosun", "--json", "plan", "import"],
            Out::ok(
                r#"{"ok":true,"data":{"epics":[{"action":"unchanged"}],"tasks":[{"action":"would-create"}]}}"#,
            ),
        );
        assert!(matches!(run(&r, Some("plan.md")), Check::Todo(m) if m.contains("1 to create")));
    }
}
