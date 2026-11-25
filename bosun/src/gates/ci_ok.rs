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

/// How a path steers ci.yml's tasks (lowercased name): always, only when its patch touches
/// build or test-runner steering, or not at all.
enum Steer {
    Always,
    Manifest(Manifest),
    No,
}

/// A manifest whose changes steer CI only in some keys (see [`manifest_steered`]).
#[derive(Clone, Copy)]
enum Manifest {
    Cargo,
    Pyproject,
    Package,
}

/// Test-runner and build config ci.yml's tasks read from the checkout besides workflows and
/// scripts: the pnpm and Turborepo task and workspace config, `.npmrc`, `.pnpmfile.*`, a Cargo
/// `build.rs`, tool configs (`*.config.{js,cjs,mjs,ts,mts,cts,json}`: vitest, vite, jest,
/// playwright, cypress, eslint), nextest config, `conftest.py`, `pytest.ini`, `tox.ini`,
/// `setup.cfg`, `tsconfig*.json`, `rust-toolchain(.toml)`, `deny.toml`, `.cargo/config(.toml)`,
/// `Makefile` and `justfile`. `Cargo.toml`, `pyproject.toml` and `package.json` steer only
/// through some keys, judged on the patch. Test files are not CI inputs: editing tests is
/// normal and weakening them is test-integrity's job.
fn steers_ci(lower: &str) -> Steer {
    let (dirs, name) = lower.rsplit_once('/').map_or(("", lower), |(d, n)| (d, n));
    let in_dir = |d: &str| dirs.split('/').any(|s| s == d);
    match name {
        "cargo.toml" => return Steer::Manifest(Manifest::Cargo),
        "pyproject.toml" => return Steer::Manifest(Manifest::Pyproject),
        "package.json" => return Steer::Manifest(Manifest::Package),
        _ => {}
    }
    let always = matches!(
        name,
        "turbo.json"
            | "pnpm-workspace.yaml"
            | ".npmrc"
            | "build.rs"
            | "conftest.py"
            | "pytest.ini"
            | "setup.cfg"
            | "tox.ini"
            | "nextest.toml"
            | "rust-toolchain"
            | "rust-toolchain.toml"
            | "deny.toml"
            | "makefile"
            | "justfile"
    ) || name.starts_with(".pnpmfile.")
        || (name.starts_with("tsconfig") && name.ends_with(".json"))
        || [".js", ".cjs", ".mjs", ".ts", ".mts", ".cts", ".json"]
            .iter()
            .any(|e| name.strip_suffix(e).is_some_and(|n| n.ends_with(".config")))
        || (matches!(name, "config.toml" | "config") && in_dir(".cargo"))
        || (name == "nextest.toml" && in_dir(".config"));
    if always { Steer::Always } else { Steer::No }
}

/// Whether the changed lines of `patch` touch what `kind` steers. Cargo: `autotests`,
/// `autobins`, `[[test]]`, `test =`, `harness`, `build =`, `[profile`. pyproject: `[tool.pytest`.
/// package.json: a line inside the `"scripts"` block, or a `"key": "value with spaces"` line
/// (a script-looking entry whose block opener the patch does not show). A key changed under a
/// section whose header is outside the hunk is not seen (residual, spec 08).
fn manifest_steered(kind: Manifest, patch: &str) -> bool {
    let mut in_scripts = false;
    for line in patch.lines() {
        if line.starts_with("@@") {
            in_scripts = false;
        }
        let (changed, text) = match line.chars().next() {
            Some('+') | Some('-') => (true, line[1..].trim()),
            _ => (false, line.trim()),
        };
        match kind {
            Manifest::Cargo => {
                if (changed || line.starts_with("@@"))
                    && ["autotests", "autobins", "[[test]]", "harness", "[profile"]
                        .iter()
                        .any(|k| text.contains(k))
                    || changed && (text.starts_with("test =") || text.starts_with("build ="))
                {
                    return true;
                }
            }
            Manifest::Pyproject => {
                if (changed || line.starts_with("@@")) && text.contains("[tool.pytest") {
                    return true;
                }
            }
            Manifest::Package => {
                if text.contains("\"scripts\"") && text.contains('{') {
                    if changed {
                        return true;
                    }
                    in_scripts = true;
                } else if in_scripts && text.starts_with('}') {
                    in_scripts = false;
                } else if changed {
                    if in_scripts {
                        return true;
                    }
                    // `"name": "tsc --noEmit"`: a value with a space is a command, not a version.
                    if let Some((k, v)) = text.split_once("\":") {
                        let v = v.trim().trim_end_matches(',').trim_matches('"');
                        if k.starts_with('"') && v.contains(' ') && !v.starts_with('{') {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// The first changed path (or the path it was renamed from) that steers what the PR's own ci.yml
/// run does from the checkout: anything under `.github/` (workflows, `.github/scripts/**`,
/// `.github/actions/**`), under a local action ci.yml uses (`local`, from [`local_actions`]),
/// anything under a `scripts/` directory at any depth, any path of the built-in R-ci rule, and
/// the build and test-runner config of [`steers_ci`]. A manifest (`Cargo.toml`, `pyproject.toml`,
/// `package.json`) counts when its patch touches steering keys; here, with no patch, it counts
/// (fail closed). A `pull_request` run executes the PR's copies of these, so the PR could make
/// its own `ci-ok` green: the mirror and the CI signer refuse it.
pub fn ci_input_changed<'a>(
    files: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    local: &[String],
) -> Option<String> {
    ci_input_changed_patched(files.into_iter().map(|(p, prev)| (p, prev, None)), local)
}

/// [`ci_input_changed`] with each file's patch (`None`: absent, a manifest then counts).
pub fn ci_input_changed_patched<'a>(
    files: impl IntoIterator<Item = (&'a str, Option<&'a str>, Option<&'a str>)>,
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
    let hit = |p: &str, patch: Option<&str>| {
        let lower = p.trim_start_matches("./").to_ascii_lowercase();
        lower.starts_with(".github/")
            || local.iter().any(|d| {
                let d = d.to_ascii_lowercase();
                lower == d || lower.starts_with(&format!("{d}/"))
            })
            // ci.yml runs folder scripts too (`node scripts/affected-combos.mjs` in hull,
            // `bash scripts/check-index.sh` in decisions): `scripts/` at any depth.
            || lower.split('/').rev().skip(1).any(|seg| seg == "scripts")
            || match steers_ci(&lower) {
                Steer::Always => true,
                Steer::Manifest(k) => patch.is_none_or(|t| manifest_steered(k, t)),
                Steer::No => false,
            }
            || r_ci(p)
    };
    files.into_iter().find_map(|(path, prev, patch)| {
        // A rename source has no patch of its own: a manifest there counts.
        if hit(path, patch) {
            Some(path)
        } else {
            prev.filter(|p| hit(p, None))
        }
        .map(str::to_string)
    })
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
    let reported = gh.get_pull(n)?.map_or(0, |p| p.changed_files);
    if files.len() >= PULL_FILES_CAP || (files.len() as u64) < reported {
        return Err(refuse(
            format!(
                "PR #{n} lists {} files, GitHub's cap for the files API ({PULL_FILES_CAP}): a changed CI input could be hidden, so its ci-ok cannot be trusted for the CI signer",
                files.len()
            ),
            "split the PR below 3000 files, or approve it with the orchestrator-held key (`bosun approve <pr>@<head> --sign-key <pem> --bootstrap`) or ask the owner",
        ));
    }
    let changed = (files.iter()).map(|f| {
        (
            f.filename.as_str(),
            f.previous_filename.as_deref(),
            f.patch.as_deref(),
        )
    });
    if let Some(p) = ci_input_changed_patched(changed, &local) {
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
    fn ci_inputs_are_build_and_test_runner_config_not_test_files() {
        let hit = |p: &str| ci_input_changed([(p, None)], &[]);
        for p in [
            "turbo.json",
            "hull/turbo.json",
            "pnpm-workspace.yaml",
            "site/.npmrc",
            "bosun/build.rs",
            ".pnpmfile.cjs",
            "vitest.config.ts",
            "web/vite.config.mjs",
            "jest.config.json",
            "playwright.config.ts",
            "cypress.config.cts",
            "conftest.py",
            "pytest.ini",
            "setup.cfg",
            "tox.ini",
            ".config/nextest.toml",
            "nextest.toml",
            "tsconfig.json",
            "web/tsconfig.base.json",
            ".cargo/config.toml",
            "x/.cargo/config",
            "rust-toolchain.toml",
            "rust-toolchain",
            "deny.toml",
            "Makefile",
            "justfile",
        ] {
            assert!(hit(p).is_some(), "{p}");
        }
        for p in [
            "bosun/src/a.rs",
            "bosun/tests/cli_a.rs",
            "tests/a.rs",
            "tests/common/mod.rs",
            "hull/tests/render.test.mjs",
            "web/src/__tests__/a.ts",
            "web/src/a.spec.ts",
            "crates/x/src/a_test.go",
            "tests/test_a.py",
            "Cargo.lock",
            "pnpm-lock.yaml",
            "docs/testing.md",
            "src/contest.rs",
            "web/src/package.jsonc.md",
            "pyproject.tomlx",
        ] {
            assert!(hit(p).is_none(), "{p}");
        }
        // A rename away from a config counts; from a test file does not.
        assert!(ci_input_changed([("src/a.rs", Some("vitest.config.ts"))], &[]).is_some());
        assert!(ci_input_changed([("src/a.rs", Some("tests/old.rs"))], &[]).is_none());
    }

    #[test]
    fn manifests_count_only_when_their_patch_touches_build_or_test_steering() {
        let hit = |p: &str, patch: Option<&str>| {
            ci_input_changed_patched([(p, None, patch)], &[]).is_some()
        };
        // No patch (too large): fail closed.
        for p in ["Cargo.toml", "x/pyproject.toml", "web/package.json"] {
            assert!(hit(p, None), "{p}");
        }
        let cargo = |l: &str| format!("@@ -1,3 +1,3 @@\n [package]\n-{l}\n+{l}2");
        for l in [
            "autotests = false",
            "autobins = false",
            "[[test]]",
            "harness = false",
            "test = false",
            "build = \"build.rs\"",
            "[profile.test]",
        ] {
            assert!(hit("Cargo.toml", Some(&cargo(l))), "{l}");
        }
        for l in ["version = \"1.2.3\"", "serde = \"=1.0.1\"", "name = \"x\""] {
            assert!(!hit("bosun/Cargo.toml", Some(&cargo(l))), "{l}");
        }
        assert!(hit(
            "pyproject.toml",
            Some("@@ -1 +1 @@\n+[tool.pytest.ini_options]")
        ));
        assert!(!hit(
            "pyproject.toml",
            Some("@@ -1 +1 @@\n-version = \"1\"\n+version = \"2\"")
        ));
        // package.json: dependency bumps no, scripts yes.
        let dep = "@@ -5,3 +5,3 @@\n   \"dependencies\": {\n-    \"left-pad\": \"1.0.0\",\n+    \"left-pad\": \"1.0.1\",\n   }";
        assert!(!hit("package.json", Some(dep)));
        let bump = "@@ -2 +2 @@\n-  \"version\": \"1.0.0\",\n+  \"version\": \"1.0.1\",";
        assert!(!hit("package.json", Some(bump)));
        let scripts = "@@ -5,3 +5,3 @@\n   \"scripts\": {\n-    \"test\": \"vitest\",\n+    \"test\": \"true\",\n   }";
        assert!(hit("package.json", Some(scripts)));
        let add = "@@ -5,3 +5,4 @@\n+  \"scripts\": {\n+    \"ci\": \"echo ok\"\n+  },";
        assert!(hit("package.json", Some(add)));
        // The opener is outside the hunk: a command-looking value still counts.
        let inner =
            "@@ -9,2 +9,2 @@\n-    \"lint\": \"oxlint .\",\n+    \"lint\": \"true && echo\",";
        assert!(hit("package.json", Some(inner)));
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
