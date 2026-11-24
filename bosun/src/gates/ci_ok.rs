//! `bosun gate ci-ok --pr N` (spec §8, keyless fast lane; §15): mirrors the `ci-ok` job of the
//! newest `.github/workflows/ci.yml` run on the PR head as a `bosun-ci-ok` check run of the gate
//! App, the only CI result the CI signer counts (`approve --as signer`). Its own name keeps it
//! apart from the required `ci-ok`: it can never satisfy or block that check, whatever the
//! ruleset pins. Any workflow can post a
//! GitHub Actions check run named `ci-ok` (a PR's new file with a job of that name included), so
//! this reads the run's workflow file from the API and mirrors ci.yml's only. It never posts a
//! status: the ruleset's `ci-ok` stays GitHub Actions'. Run by signer.yml's `mirror` job (App key
//! in `bosun-app`, default-branch code) before `sign`.

use std::path::Path;

use serde_json::json;

use crate::app::App;
use crate::config::CheckMode;
use crate::github::gate::APP_TOKEN_ENV;
use crate::github::{GateRun, Github, Job, LiveGithub, RunState, ShaRun};
use crate::output::{BosunError, CmdResult, Outcome};

/// The mirrored check run's name; the job it mirrors is ci.yml's `ci-ok`.
pub const CONTEXT: &str = "bosun-ci-ok";
/// The job of ci.yml that is mirrored.
pub const CI_JOB: &str = "ci-ok";
/// The one workflow whose `ci-ok` job is mirrored.
pub const CI_WORKFLOW: &str = ".github/workflows/ci.yml";

/// Whether a changed file (or the file it was renamed from) is a workflow the mirror trusts: a
/// PR's `pull_request` run executes the PR's own copy of ci.yml, so a PR that changes it could
/// make its own ci-ok green.
pub fn touches_trusted_workflow(path: &str, previous: Option<&str>) -> bool {
    std::iter::once(path)
        .chain(previous)
        .any(|p| p.trim_start_matches("./").eq_ignore_ascii_case(CI_WORKFLOW))
}

/// The newest run of ci.yml on `sha` for a pull request (the highest id: ids only grow).
/// A run of any other file, event or head never counts.
pub fn newest_ci_run<'a>(runs: &'a [ShaRun], sha: &str) -> Option<&'a ShaRun> {
    runs.iter()
        .filter(|r| r.head_sha == sha && r.event == "pull_request")
        .filter(|r| {
            r.path == CI_WORKFLOW
                || r.path
                    .strip_prefix(CI_WORKFLOW)
                    .is_some_and(|rest| rest.starts_with('@'))
        })
        .max_by_key(|r| r.id)
}

/// Whether the run's `ci-ok` job succeeded (the newest job of that name, by id). A missing job
/// and any other conclusion (failure, cancelled, skipped, neutral) are not green.
pub fn ci_ok_green(jobs: &[Job]) -> bool {
    jobs.iter()
        .filter(|j| j.name == CI_JOB)
        .max_by_key(|j| j.id)
        .is_some_and(|j| j.conclusion == "success")
}

pub fn run(dir: Option<&Path>, pr: Option<u64>) -> CmdResult {
    let n = pr
        .or_else(|| (std::env::var("BOSUN_PR").ok()).and_then(|v| v.trim().parse().ok()))
        .ok_or_else(|| {
            BosunError::usage("ci-ok needs a PR number", "pass --pr N (or set BOSUN_PR)")
        })?;
    let app = App::load(dir)?;
    let checks = &app.ctx.config.checks;
    if checks.mode == CheckMode::Statuses || checks.app_id.is_none() {
        return Err(BosunError::error(
            "the gate App's ci-ok needs `[checks] mode = \"checks\"` (or \"dual\") with the gate App: nothing was posted",
            "set `[checks] mode`, `app_id` and `app_slug` in .bosun/bosun.toml (spec section 15)",
        ));
    }
    let token = std::env::var(APP_TOKEN_ENV)
        .ok()
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| {
            BosunError::error(
                format!("the gate App's ci-ok needs the App token in {APP_TOKEN_ENV}: nothing was posted"),
                format!(
                    "run the job in the `bosun-app` environment with the actions/create-github-app-token step and pass its token as {APP_TOKEN_ENV}"
                ),
            )
        })?;
    let gh = app.gh.as_ref();
    let pull = gh
        .get_pull(n)?
        .filter(|p| p.state == "open")
        .ok_or_else(|| {
            BosunError::error(format!("PR #{n} is not an open PR"), "name an open PR")
        })?;
    let head = pull.head_sha.clone();
    let files = gh.pull_files(n)?;
    if (files.iter()).any(|f| touches_trusted_workflow(&f.filename, f.previous_filename.as_deref()))
    {
        return Err(BosunError::error(
            format!("PR #{n} changes {CI_WORKFLOW}, whose own run would be mirrored: nothing was posted"),
            "the CI signer does not cover a PR that changes ci.yml: approve it with the orchestrator-held key (`bosun approve <pr>@<head> --sign-key <pem> --bootstrap`) or ask the owner",
        )
        .with_data(json!({"pr": n, "head_sha": head})));
    }
    let runs = gh.workflow_runs_of(&head)?;
    let Some(ci) = newest_ci_run(&runs, &head) else {
        return Err(BosunError::error(
            format!("no {CI_WORKFLOW} run for a pull request on {head}: nothing was posted"),
            "wait for ci.yml to run on the PR head (a push or reopen starts it), then ask again",
        )
        .with_data(json!({"pr": n, "head_sha": head})));
    };
    if ci.status != "completed" {
        return Err(BosunError::error(
            format!(
                "{CI_WORKFLOW} run {} on {head} is {}: nothing was posted",
                ci.id, ci.status
            ),
            "wait for ci.yml to finish on the PR head, then ask again",
        )
        .with_data(json!({"pr": n, "head_sha": head, "run_id": ci.id})));
    }
    let green = ci_ok_green(&gh.run_jobs(ci.id)?);
    let conclusion = if green { "success" } else { "failure" };
    let title = format!(
        "{CONTEXT} {conclusion} (ci.yml run {}'s ci-ok, for the CI signer)",
        ci.id
    );
    let summary = format!(
        "The ci-ok job of {CI_WORKFLOW} run {} on this head: {conclusion}. {}",
        ci.id, ci.html_url
    );
    let client = LiveGithub::with_token(token.trim(), &app.ctx.repo.owner, &app.ctx.repo.name);
    let id = client.create_check_run(&GateRun {
        name: CONTEXT,
        head_sha: &head,
        state: if green {
            RunState::Success
        } else {
            RunState::Failure
        },
        title: &title,
        summary: &summary,
    })?;
    Ok(Outcome::data(
        json!({"pr": n, "head_sha": head, "run_id": ci.id,
        "conclusion": conclusion, "check_run_id": id}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: u64, path: &str, event: &str, sha: &str) -> ShaRun {
        ShaRun {
            id,
            path: path.into(),
            event: event.into(),
            head_sha: sha.into(),
            status: "completed".into(),
            html_url: String::new(),
        }
    }

    fn job(id: u64, name: &str, conclusion: &str) -> Job {
        Job {
            id,
            name: name.into(),
            run_id: 1,
            conclusion: conclusion.into(),
            html_url: String::new(),
        }
    }

    #[test]
    fn only_ci_yml_on_a_pull_request_of_this_head_counts_newest_first() {
        let runs = vec![
            run(1, CI_WORKFLOW, "pull_request", "a"),
            run(5, ".github/workflows/evil.yml", "pull_request", "a"),
            run(3, CI_WORKFLOW, "pull_request", "a"),
            run(9, CI_WORKFLOW, "push", "a"),
            run(8, CI_WORKFLOW, "pull_request", "b"),
            run(4, ".github/workflows/ci.yml.bak", "pull_request", "a"),
        ];
        assert_eq!(newest_ci_run(&runs, "a").map(|r| r.id), Some(3));
        let at = vec![run(
            2,
            ".github/workflows/ci.yml@refs/pull/1/merge",
            "pull_request",
            "a",
        )];
        assert_eq!(newest_ci_run(&at, "a").map(|r| r.id), Some(2));
        assert!(newest_ci_run(&runs, "c").is_none());
    }

    #[test]
    fn only_a_successful_ci_ok_job_is_green() {
        assert!(ci_ok_green(&[
            job(1, "detect", "failure"),
            job(2, "ci-ok", "success")
        ]));
        for c in ["failure", "cancelled", "skipped", "neutral", ""] {
            assert!(!ci_ok_green(&[job(2, "ci-ok", c)]), "{c}");
        }
        assert!(!ci_ok_green(&[job(1, "detect", "success")]));
        // A re-run's newer job decides.
        assert!(!ci_ok_green(&[
            job(1, "ci-ok", "success"),
            job(7, "ci-ok", "failure")
        ]));
    }
}
