//! Bind verdict records to the verdict workflow run (issue #39, spec section 8).
//!
//! Every workflow comments as `github-actions[bot]`, so the author alone does not show that
//! `bosun verdict apply` wrote a record. The record names the run, its attempt and the verdict
//! comment it applied (`"run"`, `"attempt"`, `"applied"`). A reader counts it only if all of this
//! holds: the run is the verdict workflow's `issue_comment` run on the default branch; the attempt
//! was live when the record was written (`in_progress`, or completed with the comment inside
//! `run_started_at..updated_at`); the attempt's step `bosun verdict record <pr> <comment>`
//! succeeded (the step runs only for a comment `verdict apply --check` accepts, and exits 1
//! instead of a neutral refusal) and the record was created inside that step's
//! `started_at..completed_at`; no other record names the same run and attempt; and the applied
//! comment is unedited, on the same PR, older than the attempt, and `verdict apply`'s own
//! acceptance of it (`verdict::derive`: neutral refusal, author rules, `bot:<login>` session for
//! a bot verdict) yields exactly the record's session, verdict and head. The step name carries
//! the PR and the comment of the run's event, so a run of another PR or comment never binds.
//! A record that fails any check (no run, unreadable run, missing field, mismatch) gets
//! [`UNVERIFIED_AUTHOR`]: it counts for nothing, but a rejection or a newer record of it still
//! blocks or withdraws.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::cli::verdict::derive;
use crate::core::records::Record;
use crate::core::risk::{UNVERIFIED_AUTHOR, VERDICT_AUTHOR, apply_record, record_u64};
use crate::core::verdict::{Verdict, comment_model, comment_relayed_by, effective};
use crate::github::{Comment, Github, RunJob, WorkflowRun};
use crate::time::parse_rfc3339;

/// The workflow file whose runs write verdict records.
pub const VERDICT_WORKFLOW: &str = ".github/workflows/verdict.yml";
/// The one job of that workflow and the step that writes the record.
const JOB: &str = "verdict";
/// The step that writes the record; its evaluated name is `<STEP> <pr> <comment id>`.
const STEP: &str = "bosun verdict record";

#[derive(Default)]
struct Cache {
    runs: HashMap<(String, u64), Option<WorkflowRun>>,
    attempts: HashMap<(String, u64, u64), Option<WorkflowRun>>,
    jobs: HashMap<(String, u64, u64), Option<Vec<RunJob>>>,
}

/// Each run, attempt and job list is read once per process and repository; `None`: unreadable
/// or missing.
static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn cached<K: std::hash::Hash + Eq + Clone, V: Clone>(
    pick: impl Fn(&mut Cache) -> &mut HashMap<K, Option<V>>,
    key: K,
    what: &str,
    read: impl FnOnce() -> crate::app::Result<Option<V>>,
) -> Option<V> {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = pick(guard.get_or_insert_with(Cache::default));
    map.entry(key)
        .or_insert_with(|| match read() {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "bosun: {what} is unreadable, its records do not count: {}",
                    e.message
                );
                None
            }
        })
        .clone()
}

fn repo_key(gh: &dyn Github) -> String {
    format!("{}/{}", gh.repo_owner(), gh.repo_name())
}

/// Whether `run` is the verdict workflow's `issue_comment` run on `default_branch`.
pub fn run_binds(run: &WorkflowRun, default_branch: &str) -> bool {
    let path_ok = run.path == VERDICT_WORKFLOW
        || run.path == format!("{VERDICT_WORKFLOW}@refs/heads/{default_branch}");
    path_ok
        && run.event == "issue_comment"
        && !default_branch.is_empty()
        && run.head_branch == default_branch
}

/// Whether the attempt was live at `at` (RFC 3339): `in_progress` (no end yet, the start is the
/// lower bound), or completed with `at` in `run_started_at..=updated_at`. Queued, waiting and
/// every other status never bind.
pub fn attempt_live(attempt: &WorkflowRun, at: &str) -> bool {
    let (Some(at), Some(from)) = (parse_rfc3339(at), parse_rfc3339(&attempt.run_started_at)) else {
        return false;
    };
    if at < from {
        return false;
    }
    match attempt.status.as_str() {
        "in_progress" => true,
        "completed" => parse_rfc3339(&attempt.updated_at).is_some_and(|to| at <= to),
        _ => false,
    }
}

/// Whether the record's verdict is what `verdict apply` derives from the comment `a` it names.
fn applied_yields(
    gh: &dyn Github,
    a: &Comment,
    record: &Record,
    pr: &str,
    attempt_start: &str,
) -> bool {
    let Record::Verdict {
        session,
        head_sha,
        approved,
        confidence,
        major,
        model,
        relayed_by,
    } = record
    else {
        return false;
    };
    let (Some(made), Some(start)) = (parse_rfc3339(&a.created_at), parse_rfc3339(attempt_start))
    else {
        return false;
    };
    if !a.unedited() || made > start || a.issue_url.rsplit('/').next() != Some(pr) {
        return false;
    }
    // The acceptance of `verdict apply`, not a copy of it. A refused comment, or a bot verdict
    // (recorded under `bot:<login>`, which never counts), yields no record.
    let d = match derive(gh, a, true) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "bosun: applied comment {} cannot be judged, its record does not count: {}",
                a.id, e.message
            );
            return false;
        }
    };
    if d.why.is_some() || d.bot_verdict {
        return false;
    }
    let text = d.text;
    let Some(Ok(v)) = text.map(Verdict::parse) else {
        return false;
    };
    let raw: serde_json::Value = text
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_default();
    if v.pr.to_string() != pr
        || v.head_sha != *head_sha
        || (v.verdict == "approve" && raw["chunks_reviewed"].is_number())
    {
        return false;
    }
    let mut eff = effective(&v);
    if v.escalate_risk {
        eff.approve = false;
    }
    *session == d.session
        && eff.approve == *approved
        && confidence.is_none_or(|c| c == v.confidence)
        && major.is_none_or(|m| m as usize == v.count("major"))
        && model.as_deref() == comment_model(&a.body)
        && relayed_by.as_deref() == comment_relayed_by(&a.body)
}

/// Whether the verdict job has the step `name` with no `completed_at` yet (still running).
fn step_pending(jobs: &[RunJob], name: &str) -> bool {
    jobs.iter().any(|j| {
        j.name == JOB
            && j.steps
                .iter()
                .any(|s| s.name == name && s.completed_at.trim().is_empty())
    })
}

/// Drop one cached job list so the next read asks GitHub again.
fn forget_jobs(key: &(String, u64, u64)) {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(c) = guard.as_mut() {
        c.jobs.remove(key);
    }
}

/// Whether `at` (RFC 3339) lies in the step's `started_at..=completed_at`: the record is written
/// by the recording step itself.
fn in_step(step: &crate::github::RunStep, at: &str) -> bool {
    match (
        parse_rfc3339(at),
        parse_rfc3339(&step.started_at),
        parse_rfc3339(&step.completed_at),
    ) {
        (Some(at), Some(from), Some(to)) => from <= at && at <= to,
        _ => false,
    }
}

/// Give every verdict record of `comments` that is not bound to a verdict workflow run the author
/// [`UNVERIFIED_AUTHOR`]. Call it after `record_author`, before the readers.
pub fn bind_records(gh: &dyn Github, comments: &mut [Comment]) {
    let key = |c: &Comment| Some((record_u64(&c.body, "run")?, record_u64(&c.body, "attempt")?));
    let is_record = |c: &Comment| {
        c.user.eq_ignore_ascii_case(VERDICT_AUTHOR) && apply_record(&c.body).is_some()
    };
    // At most one record may name a (run, attempt): if two do, neither counts.
    let mut named: HashMap<(u64, u64), usize> = HashMap::new();
    for c in comments.iter().filter(|c| is_record(c)) {
        if let Some(k) = key(c) {
            *named.entry(k).or_default() += 1;
        }
    }
    let mut branch: Option<Option<String>> = None;
    for c in comments.iter_mut() {
        if !is_record(c) {
            continue;
        }
        let branch = branch.get_or_insert_with(|| crate::app::default_branch(gh).ok());
        let bound = match (key(c), branch, apply_record(&c.body)) {
            (Some(k), Some(b), Some(rec)) if named.get(&k) == Some(&1) => {
                let applied = record_u64(&c.body, "applied");
                verify(gh, c, &rec, k, (b, applied))
            }
            _ => false,
        };
        if !bound {
            c.user = UNVERIFIED_AUTHOR.to_string();
        }
    }
}

fn verify(
    gh: &dyn Github,
    c: &Comment,
    rec: &Record,
    (run, attempt): (u64, u64),
    (branch, applied): (&str, Option<u64>),
) -> bool {
    let repo = repo_key(gh);
    let Some(applied) = applied else {
        return false;
    };
    let read_run = cached(
        |m| &mut m.runs,
        (repo.clone(), run),
        &format!("verdict run {run}"),
        || gh.workflow_run(run),
    );
    if !read_run.is_some_and(|r| run_binds(&r, branch)) {
        return false;
    }
    let at = cached(
        |m| &mut m.attempts,
        (repo.clone(), run, attempt),
        &format!("verdict run {run} attempt {attempt}"),
        || gh.workflow_run_attempt(run, attempt),
    );
    let Some(at) = at.filter(|a| run_binds(a, branch) && attempt_live(a, &c.created_at)) else {
        return false;
    };
    let jobs = cached(
        |m| &mut m.jobs,
        (repo.clone(), run, attempt),
        &format!("jobs of verdict run {run} attempt {attempt}"),
        || gh.run_attempt_jobs(run, attempt).map(Some),
    );
    let pr = c.issue_url.rsplit('/').next().unwrap_or("");
    let step = format!("{STEP} {pr} {applied}");
    // A step still running has no `completed_at`: the record may be fresh, read too early. It is
    // pending, not unverified: do not keep this job list in the cache.
    if jobs.as_ref().is_some_and(|js| step_pending(js, &step)) {
        forget_jobs(&(repo, run, attempt));
        return false;
    }
    let ran = jobs.is_some_and(|js| {
        js.iter().any(|j| {
            j.name == JOB
                && j.steps.iter().any(|s| {
                    s.name == step && s.conclusion == "success" && in_step(s, &c.created_at)
                })
        })
    });
    if !ran {
        return false;
    }
    match gh.get_comment(applied) {
        Ok(Some(a)) => applied_yields(gh, &a, rec, pr, &at.run_started_at),
        Ok(None) => false,
        Err(e) => {
            eprintln!(
                "bosun: applied comment {applied} is unreadable, its record does not count: {}",
                e.message
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::RunStep;

    fn job(completed_at: &str) -> RunJob {
        RunJob {
            name: JOB.into(),
            steps: vec![RunStep {
                name: "bosun verdict record 1 2".into(),
                conclusion: String::new(),
                started_at: "2026-01-01T00:00:00Z".into(),
                completed_at: completed_at.into(),
            }],
        }
    }

    #[test]
    fn an_unset_completed_at_is_pending_and_never_cached() {
        let running = job("");
        assert!(!in_step(&running.steps[0], "2026-01-01T00:00:01Z"));
        assert!(step_pending(
            std::slice::from_ref(&running),
            "bosun verdict record 1 2"
        ));
        assert!(!step_pending(
            &[job("2026-01-01T00:00:02Z")],
            "bosun verdict record 1 2"
        ));
        let key = ("o/r-pending".to_string(), 1, 1);
        let read = cached(
            |m| &mut m.jobs,
            key.clone(),
            "jobs",
            || Ok(Some(vec![running])),
        );
        assert!(read.is_some());
        forget_jobs(&key);
        let again = cached(|m| &mut m.jobs, key.clone(), "jobs", || Ok(None));
        assert!(again.is_none(), "the pending list must not stay cached");
        forget_jobs(&key);
    }
}
