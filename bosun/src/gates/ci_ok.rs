//! `bosun gate ci-ok --pr N` (spec §8, keyless fast lane; §15): mirrors the `ci-ok` job of the
//! newest `.github/workflows/ci.yml` run of the PR (into the default branch) on its head as a
//! `bosun-ci-ok` check run of the gate App, the CI result the CI signer counts (`approve --as
//! signer`, which also re-reads [`ci_verdict`] when it signs). Its own name keeps it apart from
//! the required `ci-ok`: it can never satisfy or block that check, whatever the ruleset pins. Any
//! workflow can post a GitHub Actions check run named `ci-ok` (a PR's new file with a job of that
//! name included), so this reads the run's workflow file and pull request from the API. It never
//! posts a status. Run by signer.yml's `mirror` job (App key in `bosun-app`, default-branch
//! code) before `sign`.

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

/// GitHub's pulls/files API lists at most this many files; a list this long may be cut.
const PULL_FILES_CAP: usize = 3000;

/// The local actions ci.yml runs from the checkout (`uses: ./<dir>`), from its text.
pub fn local_actions(ci_yml: &str) -> Vec<String> {
    let mut out: Vec<String> = ci_yml
        .lines()
        .map(|l| l.trim().trim_start_matches('-').trim())
        .filter_map(|l| l.strip_prefix("uses:"))
        .map(|v| {
            v.split(" #")
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(['"', '\''])
        })
        .filter_map(|v| v.strip_prefix("./"))
        .map(|v| v.trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Paths ci.yml's tasks read from the checkout besides workflows and scripts (lowercased): a
/// `package.json` (its `scripts` are what `pnpm run ci` runs), the task and workspace config of
/// pnpm and Turborepo, `.npmrc`, a Cargo `build.rs` (it runs at build time) and test files (the PR
/// could weaken the tests its own CI runs): a `tests`, `test` or `__tests__` directory at any
/// depth, `*.test.*`, `*.spec.*`, `*_test.<ext>` and `test_*.py`.
fn steers_ci(lower: &str) -> bool {
    let (dirs, name) = lower.rsplit_once('/').map_or(("", lower), |(d, n)| (d, n));
    let stem = name.split('.').next().unwrap_or(name);
    matches!(
        name,
        "package.json" | "turbo.json" | "pnpm-workspace.yaml" | ".npmrc" | "build.rs"
    ) || dirs
        .split('/')
        .any(|seg| matches!(seg, "tests" | "test" | "__tests__"))
        || name.contains(".test.")
        || name.contains(".spec.")
        || (stem.ends_with("_test") && name.contains('.'))
        || (name.starts_with("test_") && name.ends_with(".py"))
}

/// The first changed path (or the path it was renamed from) that steers what the PR's own ci.yml
/// run does from the checkout: anything under `.github/` (workflows, `.github/scripts/**` such as
/// the affected-folder detection, `.github/actions/**`), under a local action ci.yml uses
/// (`local`, from [`local_actions`], e.g. `hoard/**`), anything under a `scripts/` directory at
/// any depth (folder scripts ci.yml runs), or any path of the built-in R-ci rule
/// (every `action.yml`, toolchain and tool configs, `deny.toml`,
/// `Makefile`, ...; the built-in rule, never the repo's risk.toml, so a base config cannot narrow
/// it). A `pull_request` run executes the PR's copies of these, so the PR could make its own
/// `ci-ok` green: the mirror and the CI signer refuse it.
pub fn ci_input_changed<'a>(
    files: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    local: &[String],
) -> Option<String> {
    let rules = crate::core::risk::Rules::defaults();
    let r_ci = |p: &str| {
        let f = crate::core::risk::ChangedFile {
            path: p.to_string(),
            previous_path: None,
            added_lines: Vec::new(),
        };
        crate::core::risk::evaluate(&rules, &[f], &[], &[])
            .iter()
            .any(|m| m.rule == "R-ci")
    };
    let hit = |p: &str| {
        let lower = p.trim_start_matches("./").to_ascii_lowercase();
        lower.starts_with(".github/")
            || local.iter().any(|d| {
                let d = d.to_ascii_lowercase();
                lower == d || lower.starts_with(&format!("{d}/"))
            })
            // ci.yml runs folder scripts too (`node scripts/affected-combos.mjs` in hull,
            // `bash scripts/check-index.sh` in decisions): `scripts/` at any depth.
            || lower.split('/').rev().skip(1).any(|seg| seg == "scripts")
            || steers_ci(&lower)
            || r_ci(p)
    };
    files
        .into_iter()
        .flat_map(|(path, prev)| std::iter::once(path).chain(prev))
        .find(|p| hit(p))
        .map(str::to_string)
}

/// The newest run of ci.yml on `sha` for PR `pr` into `base` (the highest id: ids only grow).
/// A run of any other file, event or head, and a run GitHub ties to no PR, another PR or another
/// base (a PR from the same head into a branch whose ci.yml always passes), never counts.
pub fn newest_ci_run<'a>(runs: &'a [ShaRun], sha: &str, pr: u64, base: &str) -> Option<&'a ShaRun> {
    runs.iter()
        .filter(|r| r.head_sha == sha && r.event == "pull_request")
        .filter(|r| {
            r.path == CI_WORKFLOW
                || r.path
                    .strip_prefix(CI_WORKFLOW)
                    .is_some_and(|rest| rest.starts_with('@'))
        })
        .filter(|r| (r.pull_requests.iter()).any(|p| p.number == pr && p.base.git_ref == base))
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

/// What ci.yml says about PR `n` at `head`, read fresh from the Actions API.
pub struct CiVerdict {
    pub run_id: u64,
    pub green: bool,
    pub html_url: String,
}

/// The `ci-ok` of the newest ci.yml run of PR `n` at `head` (into the default branch), or why it
/// cannot be judged: the PR changes what ci.yml runs from the checkout ([`ci_input_changed`]),
/// ci.yml is not on the default branch (fail closed), no such run, or the run is not finished.
/// Shared by the mirror and the CI signer, which re-reads it when it signs, so a stale green
/// `bosun-ci-ok` (ci.yml re-run red since) never counts.
pub fn ci_verdict(gh: &dyn Github, n: u64, head: &str) -> crate::github::Result<CiVerdict> {
    let refuse = |msg: String, fix: &str| {
        BosunError::error(msg, fix).with_data(json!({"pr": n, "head_sha": head}))
    };
    let branch = crate::app::default_branch(gh)?;
    let Some(ci_yml) = gh.file_at(CI_WORKFLOW, &format!("refs/heads/{branch}"))? else {
        return Err(refuse(
            format!(
                "{CI_WORKFLOW} is not on `{branch}`: what it runs from the checkout is unknown"
            ),
            "the CI signer needs ci.yml on the default branch; approve with the orchestrator-held key or ask the owner",
        ));
    };
    let local = local_actions(&ci_yml);
    let files = gh.pull_files(n)?;
    // GitHub's files API lists at most 3000 files: a full list may hide a changed CI input.
    if files.len() >= PULL_FILES_CAP {
        return Err(refuse(
            format!(
                "PR #{n} lists {} files, GitHub's cap for the files API ({PULL_FILES_CAP}): a changed CI input could be hidden, so its ci-ok cannot be trusted for the CI signer",
                files.len()
            ),
            "split the PR below 3000 files, or approve it with the orchestrator-held key (`bosun approve <pr>@<head> --sign-key <pem> --bootstrap`) or ask the owner",
        ));
    }
    let changed = (files.iter()).map(|f| (f.filename.as_str(), f.previous_filename.as_deref()));
    if let Some(p) = ci_input_changed(changed, &local) {
        return Err(refuse(
            format!(
                "PR #{n} changes {p}, which its own {CI_WORKFLOW} run executes from the checkout (`.github/**`, local actions {local:?}, R-ci paths): its ci-ok cannot be trusted for the CI signer"
            ),
            "the CI signer does not cover such a PR: approve it with the orchestrator-held key (`bosun approve <pr>@<head> --sign-key <pem> --bootstrap`) or ask the owner",
        ));
    }
    let runs = gh.workflow_runs_of(head)?;
    let Some(ci) = newest_ci_run(&runs, head, n, &branch) else {
        return Err(refuse(
            format!("no {CI_WORKFLOW} run of PR #{n} into `{branch}` on {head}"),
            "wait for ci.yml to run on the PR head (a push or reopen starts it), then ask again",
        ));
    };
    if ci.status != "completed" {
        return Err(refuse(
            format!("{CI_WORKFLOW} run {} on {head} is {}", ci.id, ci.status),
            "wait for ci.yml to finish on the PR head, then ask again",
        ));
    }
    Ok(CiVerdict {
        run_id: ci.id,
        green: ci_ok_green(&gh.run_jobs(ci.id)?),
        html_url: ci.html_url.clone(),
    })
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
    let ci = ci_verdict(gh, n, &head).map_err(|mut e| {
        e.message = format!("{}: nothing was posted", e.message);
        e
    })?;
    let conclusion = if ci.green { "success" } else { "failure" };
    let title = format!(
        "{CONTEXT} {conclusion} (ci.yml run {}'s ci-ok, for the CI signer)",
        ci.run_id
    );
    let summary = format!(
        "The ci-ok job of {CI_WORKFLOW} run {} on this head: {conclusion}. {}",
        ci.run_id, ci.html_url
    );
    let client = LiveGithub::with_token(token.trim(), &app.ctx.repo.owner, &app.ctx.repo.name);
    let id = client.create_check_run(&GateRun {
        name: CONTEXT,
        head_sha: &head,
        state: if ci.green {
            RunState::Success
        } else {
            RunState::Failure
        },
        title: &title,
        summary: &summary,
    })?;
    Ok(Outcome::data(
        json!({"pr": n, "head_sha": head, "run_id": ci.run_id,
        "conclusion": conclusion, "check_run_id": id}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{RunPull, RunPullBase};

    fn run(id: u64, path: &str, event: &str, sha: &str) -> ShaRun {
        ShaRun {
            id,
            path: path.into(),
            event: event.into(),
            head_sha: sha.into(),
            status: "completed".into(),
            html_url: String::new(),
            pull_requests: vec![RunPull {
                number: 1,
                base: RunPullBase {
                    git_ref: "main".into(),
                },
            }],
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
        assert_eq!(newest_ci_run(&runs, "a", 1, "main").map(|r| r.id), Some(3));
        let at = vec![run(
            2,
            ".github/workflows/ci.yml@refs/pull/1/merge",
            "pull_request",
            "a",
        )];
        assert_eq!(newest_ci_run(&at, "a", 1, "main").map(|r| r.id), Some(2));
        assert!(newest_ci_run(&runs, "c", 1, "main").is_none());
    }

    #[test]
    fn a_run_for_another_pr_or_base_never_counts() {
        let mut other = run(9, CI_WORKFLOW, "pull_request", "a");
        other.pull_requests[0].base.git_ref = "evil".into();
        let mut none = run(8, CI_WORKFLOW, "pull_request", "a");
        none.pull_requests.clear();
        let runs = vec![run(1, CI_WORKFLOW, "pull_request", "a"), other, none];
        assert_eq!(newest_ci_run(&runs, "a", 1, "main").map(|r| r.id), Some(1));
        assert!(newest_ci_run(&runs, "a", 2, "main").is_none());
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

    #[test]
    fn local_actions_and_ci_inputs() {
        let yml = "      - uses: ./hoard\n      - uses: actions/checkout@abc # v4\n  - uses: './x/y/' # c\n";
        assert_eq!(local_actions(yml), ["hoard", "x/y"]);
        let local = local_actions(yml);
        let hit = |p: &str, prev: Option<&str>| ci_input_changed([(p, prev)], &local);
        assert!(hit(CI_WORKFLOW, None).is_some());
        assert!(hit(".github/scripts/affected.sh", None).is_some());
        assert!(hit("hoard/action.yml", None).is_some());
        assert!(hit("x/y/z.js", None).is_some());
        assert!(hit("a.rs", Some(".github/workflows/ci.yml")).is_some());
        assert!(hit("hoarder/a.rs", None).is_none());
        for p in [
            "scripts/x.sh",
            "hull/scripts/a.mjs",
            "tools/a/action.yml",
            "deny.toml",
        ] {
            assert!(hit(p, None).is_some(), "{p}");
        }
        assert!(hit("bosun/src/a.rs", None).is_none());
        assert!(hit("docs/scripts.md", None).is_none());
    }

    #[test]
    fn ci_inputs_cover_package_scripts_task_config_build_scripts_and_tests() {
        let hit = |p: &str| ci_input_changed([(p, None)], &[]);
        for p in [
            "package.json",
            "hull/package.json",
            "apps/web/package.json",
            "turbo.json",
            "hull/turbo.json",
            "pnpm-workspace.yaml",
            "site/pnpm-workspace.yaml",
            ".npmrc",
            "site/.npmrc",
            "build.rs",
            "bosun/build.rs",
            "bosun/tests/cli_a.rs",
            "tests/common/mod.rs",
            "hull/tests/render.test.mjs",
            "npm/keel/test/launcher.test.js",
            "web/src/__tests__/a.ts",
            "web/src/a.spec.ts",
            "web/src/a.test.tsx",
            "crates/x/src/a_test.go",
        ] {
            assert!(hit(p).is_some(), "{p}");
        }
        assert!(ci_input_changed([("src/a.rs", Some("tests/old.rs"))], &[]).is_some());
        for p in [
            "bosun/src/a.rs",
            "docs/testing.md",
            "src/contest.rs",
            "src/latest/a.rs",
            "web/src/package.jsonc.md",
        ] {
            assert!(hit(p).is_none(), "{p}");
        }
    }

    /// Parity with the real ci.yml: every `uses: ./` line is found, so a new local action counts
    /// without a code change.
    #[test]
    fn this_repos_ci_yml_local_actions_are_found() {
        let yml = include_str!("../../../.github/workflows/ci.yml");
        let found = local_actions(yml);
        assert!(found.contains(&"hoard".to_string()), "{found:?}");
        for l in yml.lines().filter(|l| l.contains("uses: ./")) {
            let dir = (l.split("uses: ./").nth(1).unwrap())
                .split_whitespace()
                .next()
                .unwrap();
            assert!(
                found.contains(&dir.trim_end_matches('/').to_string()),
                "{l}: {found:?}"
            );
        }
    }
}
