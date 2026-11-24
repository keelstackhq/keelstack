//! `bosun test-integrity [--base REF] [--pr N]` and `bosun gate test-integrity`. Works offline,
//! except that `--pr` reads the PR body and the linked task's `allow` from GitHub.

use std::path::Path;

use serde_json::json;

use crate::app::App;
use crate::cli::IntegrityArgs;
use crate::cli::gate::{repo_root, resolve_ci};
use crate::config::Config;
use crate::core::meta::{ALLOW_KINDS, linked_issues, parse_task_meta};
use crate::core::touch::{Changed, outside, plan_tick_file};
use crate::gates::integrity::{self, Grant, Request};
use crate::output::{BosunError, CmdResult};

/// Split `BOSUN_INTEGRITY_ALLOW` into kinds.
pub(crate) fn parse_allow(text: &str) -> Vec<String> {
    text.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn run(dir: Option<&Path>, args: &IntegrityArgs) -> CmdResult {
    match args.pr {
        Some(n) => from_pr(dir, args.base.as_deref(), n),
        None => check(dir, args.base.as_deref()),
    }
}

/// The PR's body and its tasks' grants, in place of `BOSUN_PR_BODY` and `BOSUN_INTEGRITY_ALLOW`.
/// The tasks are the linked issues with task metadata (in body order); together their `touch`
/// must cover every path the PR changes (added files and both names of a rename too), else no
/// task lends anything: the body is the agent's. Each task's `allow` then applies only to the
/// files its own `touch` covers. A cut file list allows nothing. `data.tasks` and `data.grants`
/// say what was used, `data.task` is the first task, `data.allow` the kinds whose every finding a
/// grant allowed (what the CI gate may pass as `BOSUN_INTEGRITY_ALLOW`), `data.allow_refused`
/// why nothing was lent.
fn from_pr(dir: Option<&Path>, base: Option<&str>, n: u64) -> CmdResult {
    let app = App::load(dir)?;
    let pull = app.gh.get_pull(n)?.ok_or_else(|| {
        BosunError::error(
            format!("PR #{n} not found"),
            "pass the number of an open PR",
        )
    })?;
    let PrGrants {
        tasks,
        grants,
        refused,
    } = pr_grants(&app, &pull)?;
    let add = |data: &mut serde_json::Value| {
        let allow = flat_allow(&grants, &data["findings"]);
        data["task"] = json!(tasks.first());
        data["tasks"] = json!(tasks);
        data["grants"] = json!(
            grants
                .iter()
                .filter(|g| !g.allow.is_empty())
                .map(|g| json!({"task": g.task, "allow": g.allow}))
                .collect::<Vec<_>>()
        );
        data["allow"] = json!(allow);
        if let Some(why) = &refused {
            data["allow_refused"] = json!(why);
        }
    };
    match check_with(dir, base, &pull.body, &[], &grants, None) {
        Ok(mut out) => {
            add(&mut out.data);
            Ok(out)
        }
        Err(mut e) => {
            if let Some(data) = e.data.as_mut() {
                add(data);
            }
            Err(e)
        }
    }
}

/// The kinds a flat `BOSUN_INTEGRITY_ALLOW` may carry for this PR: a kind some grant lists whose
/// findings (at least one, of the kinds it allows) were all allowed file by file. A flat list over the same
/// diff then allows exactly what the grants allowed, never a finding outside its task's touch.
fn flat_allow(grants: &[Grant], findings: &serde_json::Value) -> Vec<String> {
    let findings = findings.as_array().cloned().unwrap_or_default();
    let covers = |kind: &str| -> &[&str] {
        match kind {
            "delete-test" => &["deleted-test", "fewer-tests"],
            "skip" => &["new-skip"],
            "only" => &["new-only"],
            "fewer-asserts" => &["fewer-asserts"],
            _ => &[],
        }
    };
    let mut out: Vec<String> = Vec::new();
    for g in grants {
        for k in &g.allow {
            let of_kind: Vec<&serde_json::Value> = findings
                .iter()
                .filter(|f| covers(k).iter().any(|c| f["kind"] == *c))
                .collect();
            // Only a kind the PR needs: a granted kind with no finding stays out, so a gate run on
            // another diff cannot use it on a file outside the granting task's touch.
            let all = !of_kind.is_empty() && of_kind.iter().all(|f| f["allowed"] == true);
            if all && !out.contains(k) {
                out.push(k.clone());
            }
        }
    }
    out
}

/// The tasks a PR closes, their grants and why none was lent (see [`from_pr`]). Shared with
/// `bosun merge`'s test-integrity gate.
#[derive(Debug, Default)]
pub(crate) struct PrGrants {
    pub tasks: Vec<u64>,
    pub grants: Vec<Grant>,
    pub refused: Option<String>,
}

pub(crate) fn pr_grants(app: &App, pull: &crate::github::Pull) -> Result<PrGrants, BosunError> {
    let files = app.gh.pull_files(pull.number)?;
    // Every path counts, added files and both names of a rename too: an add-only PR must not
    // borrow `skip` or `only` from a task whose touch it never edits. Checked as edits.
    let changed: Vec<Changed> = files
        .iter()
        .flat_map(|f| {
            (std::iter::once(&f.filename).chain(&f.previous_filename)).map(move |p| (p, f))
        })
        .map(|(p, f)| Changed {
            path: p.clone(),
            status: "modified".into(),
            previous: None,
            tick_only: plan_tick_file(
                &f.filename,
                &f.status,
                f.previous_filename.as_deref(),
                f.patch.as_deref(),
            ),
        })
        .collect();
    if (files.len() as u64) < pull.changed_files || files.len() >= 3000 {
        return Ok(PrGrants {
            refused: Some("GitHub cut the PR's file list".into()),
            ..PrGrants::default()
        });
    }
    let mut tasks = Vec::new();
    let mut grants: Vec<Grant> = Vec::new();
    for i in linked_issues(&pull.body) {
        let Some(m) = app
            .gh
            .get_issue(i)?
            // Only an open task lends its grants: a closed one (done, or a decoy) never does.
            .filter(|x| !x.pull_request && x.state == "open")
            .and_then(|x| parse_task_meta(x.body_text()))
        else {
            continue;
        };
        if m.touch.is_empty() {
            continue;
        }
        let mut allow = m.allow;
        allow.retain(|k| ALLOW_KINDS.contains(&k.as_str()));
        tasks.push(i);
        grants.push(Grant {
            task: i,
            touch: m.touch,
            allow,
        });
    }
    let union: Vec<String> = grants.iter().flat_map(|g| g.touch.clone()).collect();
    let out = outside(&changed, &union);
    if tasks.is_empty() || !out.is_empty() {
        let why = match (tasks.is_empty(), out.first()) {
            (true, _) => "the PR links no task with a touch".to_string(),
            (false, Some(f)) => format!(
                "the touch of {} does not cover {f}",
                tasks
                    .iter()
                    .map(|t| format!("#{t}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            (false, None) => "no task covers the PR".to_string(),
        };
        return Ok(PrGrants {
            tasks: Vec::new(),
            grants: Vec::new(),
            refused: Some(why),
        });
    }
    Ok(PrGrants {
        tasks,
        grants,
        refused: None,
    })
}

/// Shared by the command and the gate.
pub fn check(dir: Option<&Path>, base: Option<&str>) -> CmdResult {
    let body = std::env::var("BOSUN_PR_BODY").unwrap_or_default();
    let allow = parse_allow(&std::env::var("BOSUN_INTEGRITY_ALLOW").unwrap_or_default());
    check_with(dir, base, &body, &allow, &[], None)
}

pub(crate) fn check_with(
    dir: Option<&Path>,
    base: Option<&str>,
    body: &str,
    allow: &[String],
    grants: &[Grant],
    default: Option<&integrity::DefaultView>,
) -> CmdResult {
    let dir = dir.map(Path::to_path_buf).unwrap_or_else(|| ".".into());
    let root = repo_root(&dir)?;
    let (base, _) = resolve_ci(&root, base)?;
    let config = Config::load(&root)?;
    integrity::run(&Request {
        root: &root,
        stacks: &config.stack,
        base: &base,
        body,
        allow,
        grants,
        default,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list_splits_on_commas() {
        assert_eq!(
            parse_allow(" skip, delete-test ,,"),
            vec!["skip", "delete-test"]
        );
        assert!(parse_allow("").is_empty());
    }
}
