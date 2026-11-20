use super::{repo, work_branch};
use crate::ctx::Ctx;
use crate::error::KeelError;
use crate::exec::Cmd;
use crate::report::{Healthz, HumanStep, Smoke, Verify as Result_};
use crate::step::{Check, Step};
use crate::tools::{curl, failure, gh};
use std::time::Duration;

/// After the merge: the deploy workflow succeeded, `/healthz` answers 200, `scripts/smoke.sh` passes.
pub struct Verify;

const WORKFLOW: &str = "deploy.yml";
const SMOKE: &str = "scripts/smoke.sh";

/// Call `probe` until it says done or `wait_secs` have passed (counted in poll intervals, so a
/// test's no-op sleep does not wait).
fn poll<T>(cx: &Ctx, mut probe: impl FnMut() -> (bool, T)) -> T {
    // `plan` never waits.
    let wait = if cx.opts.dry_run {
        0
    } else {
        cx.opts.wait_secs
    };
    let step = cx.opts.poll_secs.max(1);
    let mut waited = 0;
    loop {
        let (done, v) = probe();
        if done || waited >= wait {
            return v;
        }
        (cx.sleep)(Duration::from_secs(step));
        waited += step;
    }
}

fn wait_for_first_deploy(short: &str) -> Check {
    Check::Human(HumanStep {
        id: "verify".into(),
        title: format!(
            "Wait for the first deploy (the deploy workflow has not run for merge commit {short} yet)"
        ),
        url: String::new(),
        command: "keel up".into(),
    })
}

impl Step for Verify {
    fn id(&self) -> &'static str {
        "verify"
    }

    fn check(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        let Some(f) = cx.manifest.fly().cloned() else {
            return Ok(Check::Done("no [deploy.fly]".into()));
        };
        let repo = repo(cx)?;
        let url = format!("https://{}", f.host);
        let mut v = Result_::not_run(&f.host);
        cx.verify = Some(v.clone());
        // (a) the deploy workflow on the default branch.
        let default = match gh::default_branch(cx.runner, &repo) {
            Ok(b) => b,
            Err(e) => return Ok(Check::Failed(e)),
        };
        // The run must be the one of the merged commit: an older or newer run of another commit
        // says nothing about this merge.
        let branch = match work_branch(cx) {
            Ok(b) => b,
            Err(e) => return Ok(Check::Failed(e)),
        };
        let sha = match gh::pr_view(cx.runner, &cx.dir, &repo, &branch) {
            Some(p) if !p.merge_commit.is_empty() => p.merge_commit,
            _ => {
                return Ok(Check::Failed(format!(
                    "cannot read the merge commit of the pull request of {branch}: is it merged? (`gh pr view {branch} --repo {repo} --json mergeCommit`)"
                )));
            }
        };
        let short = &sha[..sha.len().min(7)];
        let run = poll(cx, || {
            let r = gh::run_for_commit(cx.runner, &repo, WORKFLOW, &default, &sha);
            let done = match &r {
                Ok(Some(run)) => run.status == "completed",
                _ => false,
            };
            (done, r)
        });
        match run {
            Err(e) => return Ok(Check::Failed(e)),
            Ok(None) => return Ok(wait_for_first_deploy(short)),
            Ok(Some(run)) if run.status != "completed" => {
                return Ok(Check::Failed(format!(
                    "the deploy workflow for {short} is still {} after {}s: {}; rerun `keel up` later",
                    run.status, cx.opts.wait_secs, run.url
                )));
            }
            Ok(Some(run)) if run.conclusion != "success" => {
                return Ok(Check::Failed(format!(
                    "the deploy workflow for {short} {}: {}",
                    run.conclusion, run.url
                )));
            }
            Ok(Some(_)) => {}
        }
        // (b) /healthz.
        let healthz = format!("{url}/healthz");
        let p = poll(cx, || {
            let p = curl::probe(cx.runner, &healthz);
            (p.ok(), p)
        });
        v.http_status = p.status;
        if !p.ok() {
            v.healthz = Healthz::Failed;
            cx.verify = Some(v);
            return Ok(Check::Failed(format!("{healthz}: {}", p.error)));
        }
        v.healthz = Healthz::Ok;
        // (c) the project's own smoke test.
        if cx.opts.dry_run {
            // `plan` writes and runs nothing of the project's.
            v.smoke = Smoke::NotRun;
        } else if cx.dir.join(SMOKE).is_file() {
            let mut c = Cmd::new(SMOKE, &[])
                .dir(&cx.dir)
                .timeout(cx.opts.tool_timeout());
            c.env.push(("SMOKE_URL".into(), url.clone()));
            let o = cx.runner.run(&c);
            if o.code != 0 {
                v.smoke = Smoke::Failed;
                cx.verify = Some(v);
                return Ok(Check::Failed(failure(&c, &o)));
            }
            v.smoke = Smoke::Passed;
        } else {
            v.smoke = Smoke::None;
        }
        let smoke = if v.smoke == Smoke::Passed {
            "smoke passed"
        } else if v.smoke == Smoke::NotRun {
            "smoke not run in plan"
        } else {
            "no smoke script"
        };
        cx.verify = Some(v);
        Ok(Check::Done(format!("{url}/healthz 200, {smoke}")))
    }

    /// Read-only, like the check.
    fn apply(&self, cx: &mut Ctx) -> Result<Check, KeelError> {
        self.check(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::super::deploy::tests::fly_manifest;
    use super::super::testutil::*;
    use super::*;
    use crate::ctx::Opts;
    use crate::exec::{FakeRunner, Out};
    use std::cell::Cell;

    const MAIN: &str = r#"{"defaultBranchRef":{"name":"main"}}"#;

    const SHA: &str = "deadbeef1234567890";
    const MERGED: &str = r#"{"number":7,"url":"https://gh/pull/7","state":"MERGED","headRefOid":"abc123","mergeCommit":{"oid":"deadbeef1234567890"}}"#;

    fn base() -> FakeRunner {
        FakeRunner::new()
            .on(&["gh", "repo", "view"], Out::ok(MAIN))
            .on(
                &["git", "rev-parse", "--abbrev-ref", "HEAD"],
                Out::ok("keel/bootstrap\n"),
            )
            .on(&["gh", "pr", "view"], Out::ok(MERGED))
    }

    fn runs(json: &str) -> Out {
        Out::ok(json)
    }

    fn go(r: &FakeRunner, d: &std::path::Path, wait: u64) -> (Check, Option<Result_>, u32) {
        let slept = Cell::new(0);
        let sleep = |_: Duration| slept.set(slept.get() + 1);
        let opts = Opts {
            wait_secs: wait,
            poll_secs: 5,
            ..Opts::default()
        };
        let mut cx = ctx(r, d.into(), fly_manifest(), PINS, opts, &sleep, &no_stdin);
        let c = Verify.check(&mut cx).unwrap();
        (c, cx.verify.clone(), slept.get())
    }

    const OK_RUN: &str = r#"[{"status":"completed","conclusion":"success","url":"https://gh/run/1","headSha":"deadbeef1234567890"}]"#;

    #[test]
    fn no_fly_is_done_and_records_nothing() {
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
        assert_eq!(
            Verify.check(&mut cx).unwrap(),
            Check::Done("no [deploy.fly]".into())
        );
        assert!(cx.verify.is_none());
    }

    #[test]
    fn success_healthz_and_no_smoke() {
        let d = tempfile::tempdir().unwrap();
        let r = base()
            .on(&["gh", "run", "list"], runs(OK_RUN))
            .on(&["curl"], Out::ok("200"));
        let (c, v, _) = go(&r, d.path(), 0);
        assert!(matches!(c, Check::Done(_)));
        let v = v.unwrap();
        assert_eq!(
            (v.url.as_str(), v.healthz, v.http_status, v.smoke),
            (
                "https://app.example.com",
                Healthz::Ok,
                Some(200),
                Smoke::None
            )
        );
        let lines: Vec<String> = r.calls().iter().map(|c| c.line()).collect();
        assert!(lines.contains(&"gh run list --repo owner/name --workflow deploy.yml --branch main --limit 20 --json status,conclusion,url,headSha".to_string()));
        assert!(lines.contains(&"gh pr view keel/bootstrap --repo owner/name --json number,url,state,headRefOid,mergeCommit".to_string()));
        assert!(
            lines
                .last()
                .unwrap()
                .contains("https://app.example.com/healthz")
        );
    }

    #[test]
    fn smoke_script_runs_with_smoke_url() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("scripts")).unwrap();
        std::fs::write(d.path().join(SMOKE), "").unwrap();
        let r = base()
            .on(&["gh", "run", "list"], runs(OK_RUN))
            .on(&["curl"], Out::ok("200"))
            .on(&["scripts/smoke.sh"], Out::ok(""));
        let (c, v, _) = go(&r, d.path(), 0);
        assert!(matches!(c, Check::Done(_)));
        assert_eq!(v.unwrap().smoke, Smoke::Passed);
        let call = r.calls().pop().unwrap();
        assert_eq!(call.timeout, Some(Duration::from_secs(600)));
        assert_eq!(
            call.env,
            [(
                "SMOKE_URL".to_string(),
                "https://app.example.com".to_string()
            )]
        );
        // A failing smoke test fails the step.
        let r = base()
            .on(&["gh", "run", "list"], runs(OK_RUN))
            .on(&["curl"], Out::ok("200"))
            .on(&["scripts/smoke.sh"], Out::with(1, "", "boom"));
        let (c, v, _) = go(&r, d.path(), 0);
        assert!(matches!(c, Check::Failed(m) if m.contains("boom")));
        assert_eq!(v.unwrap().smoke, Smoke::Failed);
    }

    #[test]
    fn plan_never_runs_the_smoke_script() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("scripts")).unwrap();
        std::fs::write(d.path().join(SMOKE), "").unwrap();
        let r = base()
            .on(&["gh", "run", "list"], runs(OK_RUN))
            .on(&["curl"], Out::ok("200"))
            .on(&["scripts/smoke.sh"], Out::with(1, "", "must not run"));
        let opts = Opts {
            dry_run: true,
            ..Opts::default()
        };
        let mut cx = ctx(
            &r,
            d.path().into(),
            fly_manifest(),
            PINS,
            opts,
            &no_sleep,
            &no_stdin,
        );
        let c = Verify.check(&mut cx).unwrap();
        assert!(matches!(c, Check::Done(_)), "{c:?}");
        assert_eq!(cx.verify.unwrap().smoke, Smoke::NotRun);
        assert!(r.calls().iter().all(|c| c.program != SMOKE));
    }

    #[test]
    fn failed_run_is_failed_with_its_url_and_skips_healthz() {
        let d = tempfile::tempdir().unwrap();
        let r = base().on(
            &["gh", "run", "list"],
            runs(r#"[{"status":"completed","conclusion":"failure","url":"https://gh/run/9","headSha":"deadbeef1234567890"}]"#),
        );
        let (c, v, _) = go(&r, d.path(), 0);
        assert!(
            matches!(c, Check::Failed(m) if m.contains("https://gh/run/9") && m.contains("failure"))
        );
        assert_eq!(v.unwrap().healthz, Healthz::NotRun);
        assert!(r.calls().iter().all(|c| c.program != "curl"));
    }

    #[test]
    fn no_run_after_the_wait_is_a_human_step_and_polls_meanwhile() {
        let d = tempfile::tempdir().unwrap();
        let r = base().on(&["gh", "run", "list"], runs("[]"));
        let (c, _, slept) = go(&r, d.path(), 10);
        let Check::Human(h) = c else { panic!() };
        assert_eq!(h.command, "keel up");
        assert!(h.title.contains("first deploy"));
        assert_eq!(slept, 2);
    }

    #[test]
    fn only_the_run_of_the_merge_commit_counts() {
        let d = tempfile::tempdir().unwrap();
        // Newer and older runs of other commits are ignored, even a failed one.
        let others = r#"[{"status":"completed","conclusion":"failure","url":"https://gh/run/3","headSha":"1111"},{"status":"completed","conclusion":"success","url":"https://gh/run/0","headSha":"2222"}]"#;
        let r = base().on(&["gh", "run", "list"], runs(others));
        let (c, v, slept) = go(&r, d.path(), 10);
        let Check::Human(h) = c else { panic!("{c:?}") };
        assert!(h.title.contains("first deploy"));
        assert_eq!(slept, 2);
        assert_eq!(v.unwrap().healthz, Healthz::NotRun);
        // The matching run is picked from among them.
        let mixed = format!(
            "[{},{}]",
            r#"{"status":"completed","conclusion":"failure","url":"https://gh/run/3","headSha":"1111"}"#,
            r#"{"status":"completed","conclusion":"success","url":"https://gh/run/2","headSha":"deadbeef1234567890"}"#
        );
        let r = base()
            .on(&["gh", "run", "list"], runs(&mixed))
            .on(&["curl"], Out::ok("200"));
        let (c, _, _) = go(&r, d.path(), 0);
        assert!(matches!(c, Check::Done(_)), "{c:?}");
        // A run of the merge commit that is still going is polled, not failed early.
        let going =
            format!(r#"[{{"status":"in_progress","conclusion":"","url":"u","headSha":"{SHA}"}}]"#);
        let r = base().on(&["gh", "run", "list"], runs(&going));
        let (c, _, slept) = go(&r, d.path(), 10);
        assert!(matches!(c, Check::Failed(m) if m.contains("still in_progress")));
        assert_eq!(slept, 2);
    }

    #[test]
    fn an_unreadable_merge_commit_fails_without_listing_runs() {
        let d = tempfile::tempdir().unwrap();
        let r = base().on(
            &["gh", "pr", "view"],
            Out::ok(r#"{"number":7,"url":"u","state":"OPEN","headRefOid":"a","mergeCommit":null}"#),
        );
        let (c, _, _) = go(&r, d.path(), 0);
        assert!(matches!(c, Check::Failed(m) if m.contains("merge commit")));
        assert!(
            r.calls()
                .iter()
                .all(|c| c.args.get(1).map(String::as_str) != Some("run"))
        );
    }

    #[test]
    fn healthz_failure_reports_the_status() {
        let d = tempfile::tempdir().unwrap();
        let r = base()
            .on(&["gh", "run", "list"], runs(OK_RUN))
            .on(&["curl"], Out::with(22, "503", ""));
        let (c, v, slept) = go(&r, d.path(), 5);
        assert!(matches!(c, Check::Failed(m) if m == "https://app.example.com/healthz: HTTP 503"));
        let v = v.unwrap();
        assert_eq!(
            (v.healthz, v.http_status, v.smoke),
            (Healthz::Failed, Some(503), Smoke::NotRun)
        );
        assert_eq!(slept, 1);
    }
}
