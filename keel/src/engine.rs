use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::report::{Action, HumanStep, Report, Status, StepResult};
use crate::step::{Check, Step};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Up,
    Plan,
}

/// Run the steps in order. `Up`: check, apply what is todo, stop at the first human or failed
/// step. `Plan`: check only, `would-run` for work, stop at a human or failed step.
pub fn run_steps(cx: &mut Ctx, steps: &[Box<dyn Step>], mode: Mode) -> Result<Report, KeelError> {
    let command = if mode == Mode::Up { "up" } else { "plan" };
    let mut results: Vec<StepResult> = Vec::new();
    let mut human: Option<HumanStep> = None;
    let mut failed = false;
    let mut would_run = false;
    for step in steps {
        let id = step.id();
        let mut outcome = step.check(cx)?;
        if mode == Mode::Up
            && let Check::Todo(_) = outcome
        {
            outcome = match step.apply(cx)? {
                Check::Done(d) => {
                    results.push(StepResult {
                        id: id.into(),
                        action: Action::Done,
                        detail: d,
                    });
                    continue;
                }
                Check::Todo(d) => Check::Failed(format!("still not done after apply: {d}")),
                other => other,
            };
        }
        let (action, detail) = match outcome {
            Check::Done(d) => (Action::Ok, d),
            Check::Todo(d) => {
                would_run = true;
                (Action::WouldRun, d)
            }
            Check::Human(h) => {
                let d = h.title.clone();
                human = Some(h);
                (Action::Pending, d)
            }
            Check::Failed(d) => {
                failed = true;
                (Action::Failed, d)
            }
        };
        results.push(StepResult {
            id: id.into(),
            action,
            detail,
        });
        if human.is_some() || failed {
            break;
        }
    }
    for step in &steps[results.len()..] {
        results.push(StepResult {
            id: step.id().into(),
            action: Action::NotReached,
            detail: String::new(),
        });
    }
    let (status, exit_code) = if failed {
        (Status::Failed, 1)
    } else if human.is_some() {
        (Status::Human, 5)
    } else if would_run {
        (Status::Planned, 1)
    } else {
        (Status::Done, 0)
    };
    Ok(Report {
        command: command.into(),
        status,
        steps: results,
        human_step: human,
        exit_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::FakeRunner;
    use crate::manifest::Manifest;
    use crate::pins::Pins;
    use std::cell::Cell;
    use std::rc::Rc;

    /// check results in order, then apply result; counts applies.
    struct Dummy {
        id: &'static str,
        check: Check,
        apply: Check,
        applied: Rc<Cell<u32>>,
    }

    impl Step for Dummy {
        fn id(&self) -> &'static str {
            self.id
        }
        fn check(&self, _: &mut Ctx) -> Result<Check, KeelError> {
            Ok(self.check.clone())
        }
        fn apply(&self, _: &mut Ctx) -> Result<Check, KeelError> {
            self.applied.set(self.applied.get() + 1);
            Ok(self.apply.clone())
        }
    }

    fn step(id: &'static str, check: Check, apply: Check, n: &Rc<Cell<u32>>) -> Box<dyn Step> {
        Box::new(Dummy {
            id,
            check,
            apply,
            applied: n.clone(),
        })
    }

    fn human() -> Check {
        Check::Human(HumanStep {
            id: "h".into(),
            title: "do it".into(),
            url: "u".into(),
            command: "keel up".into(),
        })
    }

    fn run(steps: Vec<Box<dyn Step>>, mode: Mode) -> Report {
        let runner = FakeRunner::new();
        let sleep = |_| {};
        let stdin = || Ok(zeroize::Zeroizing::new(String::new()));
        let mut cx = Ctx {
            runner: &runner,
            dir: ".".into(),
            manifest: Manifest::default(),
            pins: Pins::default(),
            opts: Opts::default(),
            sleep: &sleep,
            stdin: &stdin,
        };
        run_steps(&mut cx, &steps, mode).unwrap()
    }

    fn actions(r: &Report) -> Vec<Action> {
        r.steps.iter().map(|s| s.action).collect()
    }

    #[test]
    fn up_all_done_exits_zero_and_skips_apply() {
        let n = Rc::new(Cell::new(0));
        let r = run(
            vec![
                step("a", Check::Done("x".into()), Check::Done("".into()), &n),
                step("b", Check::Done("y".into()), Check::Done("".into()), &n),
            ],
            Mode::Up,
        );
        assert_eq!(actions(&r), [Action::Ok, Action::Ok]);
        assert_eq!((r.status, r.exit_code, n.get()), (Status::Done, 0, 0));
    }

    #[test]
    fn up_applies_todo_and_stops_at_human() {
        let n = Rc::new(Cell::new(0));
        let r = run(
            vec![
                step("a", Check::Todo("t".into()), Check::Done("did".into()), &n),
                step("b", human(), Check::Done("".into()), &n),
                step("c", Check::Todo("t".into()), Check::Done("".into()), &n),
            ],
            Mode::Up,
        );
        assert_eq!(
            actions(&r),
            [Action::Done, Action::Pending, Action::NotReached]
        );
        assert_eq!(r.steps[0].detail, "did");
        assert_eq!((r.status, r.exit_code, n.get()), (Status::Human, 5, 1));
        assert_eq!(r.human_step.unwrap().command, "keel up");
    }

    #[test]
    fn up_apply_returning_human_or_failed() {
        let n = Rc::new(Cell::new(0));
        let r = run(
            vec![
                step("a", Check::Todo("t".into()), human(), &n),
                step("b", Check::Done("".into()), Check::Done("".into()), &n),
            ],
            Mode::Up,
        );
        assert_eq!(actions(&r), [Action::Pending, Action::NotReached]);
        assert_eq!(r.exit_code, 5);
        let r = run(
            vec![
                step(
                    "a",
                    Check::Todo("t".into()),
                    Check::Failed("boom".into()),
                    &n,
                ),
                step("b", Check::Done("".into()), Check::Done("".into()), &n),
            ],
            Mode::Up,
        );
        assert_eq!(actions(&r), [Action::Failed, Action::NotReached]);
        assert_eq!((r.status, r.exit_code), (Status::Failed, 1));
        assert!(r.human_step.is_none());
    }

    #[test]
    fn up_apply_still_todo_is_failed() {
        let n = Rc::new(Cell::new(0));
        let r = run(
            vec![step(
                "a",
                Check::Todo("t".into()),
                Check::Todo("t".into()),
                &n,
            )],
            Mode::Up,
        );
        assert_eq!(actions(&r), [Action::Failed]);
        assert!(r.steps[0].detail.contains("still not done"));
    }

    #[test]
    fn check_failure_stops_up() {
        let n = Rc::new(Cell::new(0));
        let r = run(
            vec![
                step("a", Check::Failed("no".into()), Check::Done("".into()), &n),
                step("b", Check::Done("".into()), Check::Done("".into()), &n),
            ],
            Mode::Up,
        );
        assert_eq!(actions(&r), [Action::Failed, Action::NotReached]);
        assert_eq!(n.get(), 0);
    }

    #[test]
    fn plan_never_applies_and_continues_past_todo() {
        let n = Rc::new(Cell::new(0));
        let r = run(
            vec![
                step("a", Check::Todo("t".into()), Check::Done("".into()), &n),
                step("b", Check::Todo("t".into()), Check::Done("".into()), &n),
                step("c", human(), Check::Done("".into()), &n),
                step("d", Check::Todo("t".into()), Check::Done("".into()), &n),
            ],
            Mode::Plan,
        );
        assert_eq!(
            actions(&r),
            [
                Action::WouldRun,
                Action::WouldRun,
                Action::Pending,
                Action::NotReached
            ]
        );
        assert_eq!((r.exit_code, n.get()), (5, 0));
        assert_eq!(r.command, "plan");
    }

    #[test]
    fn plan_would_run_exits_one_and_all_ok_exits_zero() {
        let n = Rc::new(Cell::new(0));
        let r = run(
            vec![
                step("a", Check::Done("".into()), Check::Done("".into()), &n),
                step("b", Check::Todo("t".into()), Check::Done("".into()), &n),
            ],
            Mode::Plan,
        );
        assert_eq!((r.status, r.exit_code), (Status::Planned, 1));
        let r = run(
            vec![step(
                "a",
                Check::Done("".into()),
                Check::Done("".into()),
                &n,
            )],
            Mode::Plan,
        );
        assert_eq!((r.status, r.exit_code), (Status::Done, 0));
    }
}
