//! Running `release publish`'s steps: the dry run, and the run that stops at the first failure
//! and then verifies (spec §9.3, §9.5).

use super::registries::{self, Outcome};
use super::{Clients, Job, Step, github, item, item_of, report};
use crate::error::HoistError;
use crate::repo::git::GithubRepo;
use crate::report::{Action, HumanStep, Report};
use std::path::Path;

fn add_step(steps: &mut Vec<HumanStep>, step: HumanStep) {
    if !steps.iter().any(|s| s.id == step.id) {
        steps.push(step);
    }
}

/// The first of `job`'s workspace dependencies that is still waiting to be published for the
/// first time (a registry step cannot succeed before it).
fn waits_on<'a>(job: &Job, step: Step, pending: &'a [String]) -> Option<&'a String> {
    if !matches!(step, Step::Npm | Step::Crates) {
        return None;
    }
    pending.iter().find(|n| job.pkg.deps.contains(n))
}

/// An npm launcher (and its platform packages) install the crate's binary: while the crate is
/// not on crates.io at this version they wait, whether the crate is held in this run (`pending`)
/// or this run publishes npm only (`--only npm`) and crates.io has no such version.
fn waits_for_crate(c: &Clients, job: &Job, pending: &[String]) -> Option<String> {
    let l = job.pkg.launcher.as_ref()?;
    let (name, v) = (&l.crate_name, &job.pkg.version.value);
    if pending.contains(name) {
        return Some(format!(
            "waits for crate {name}, which is not on crates.io yet"
        ));
    }
    if job.only.is_some_and(|o| o.crates) {
        return None;
    }
    let path = format!(
        "/api/v1/crates/{}/{}",
        crate::http::seg(name),
        crate::http::seg(v)
    );
    match c.crates.get_json(&path) {
        Ok(crate::http::Fetched::Ok(_)) => None,
        Ok(crate::http::Fetched::NotFound) => Some(format!(
            "waits for crate {name} {v}, which is not on crates.io yet"
        )),
        Ok(crate::http::Fetched::Forbidden(why)) => {
            Some(format!("cannot read crate {name} {v} on crates.io: {why}"))
        }
        Err(e) => Some(format!("cannot read crate {name} {v} on crates.io: {e}")),
    }
}

const PLATFORMS_PENDING: &str = "platform packages not published yet";

/// R27: a launcher's own npm step waits while any of its platform packages is pending (npm
/// would install a launcher whose binaries are missing).
fn waits_for_platforms(job: &Job, step: Step, platform_pending: bool) -> bool {
    platform_pending && step == Step::Npm && job.pkg.launcher.is_some()
}

/// `--dry-run`: every step from GETs only (R17: exit 0).
pub(super) fn dry_run(
    c: &Clients,
    repo: &GithubRepo,
    jobs: &[Job],
    head: &str,
) -> Result<Report, HoistError> {
    let mut items = Vec::new();
    let mut humans = Vec::new();
    let mut pending_pkgs: Vec<String> = Vec::new();
    for job in jobs {
        let mut platform_pending = false;
        let crate_wait = waits_for_crate(c, job, &pending_pkgs);
        for step in job.steps() {
            if let Some(why) = crate_wait.as_ref().filter(|_| !step.github()) {
                if !pending_pkgs.contains(&job.pkg.name) {
                    pending_pkgs.push(job.pkg.name.clone());
                }
                items.push(item(job, step, Action::Pending, why.clone()));
                continue;
            }
            if let Some(dep) = waits_on(job, step, &pending_pkgs) {
                let why = format!("waits for {dep}, which is not on its registry yet");
                // Held in turn: whatever depends on this package waits too.
                if !pending_pkgs.contains(&job.pkg.name) {
                    pending_pkgs.push(job.pkg.name.clone());
                }
                items.push(item(job, step, Action::Pending, why));
                continue;
            }
            if waits_for_platforms(job, step, platform_pending) {
                if !pending_pkgs.contains(&job.pkg.name) {
                    pending_pkgs.push(job.pkg.name.clone());
                }
                items.push(item(
                    job,
                    step,
                    Action::Pending,
                    PLATFORMS_PENDING.to_string(),
                ));
                continue;
            }
            let (action, why) = if step.github() {
                github::dry_run(&c.gh, repo, job, step, head)?
            } else {
                let (action, why, human) = registries::dry_run(c, job, step)?;
                if let Some(h) = human {
                    add_step(&mut humans, h);
                }
                if matches!(step, Step::Platform(_)) && action == Action::Pending {
                    platform_pending = true;
                }
                if action == Action::Pending && !pending_pkgs.contains(&job.pkg.name) {
                    pending_pkgs.push(job.pkg.name.clone());
                }
                (action, why)
            };
            items.push(item(job, step, action, why));
        }
    }
    Ok(report(items, humans, 0))
}

fn run_step(
    c: &Clients,
    dir: &Path,
    repo: &GithubRepo,
    job: &Job,
    step: Step,
    head: &str,
    warnings: &mut Vec<String>,
) -> Result<Outcome, HoistError> {
    if step.github() {
        github::run_step(&c.gh, &c.up, repo, job, step, head).map(Outcome::Done)
    } else {
        registries::run_step(c, dir, job, step, warnings)
    }
}

/// Every step in order; the first failure stops the run (exit 1), a package waiting on a
/// person (first publish) does not. Then verify each step that ran; human steps alone exit 2.
pub(super) fn execute(
    c: &Clients,
    dir: &Path,
    repo: &GithubRepo,
    jobs: &[Job],
    head: &str,
) -> Report {
    let mut items = Vec::new();
    let mut humans = Vec::new();
    let mut done = Vec::new(); // (item index, job, step) to verify
    let mut failed = false;
    let mut pending_pkgs: Vec<String> = Vec::new();
    for job in jobs {
        let mut platform_pending = false;
        let crate_wait = if failed {
            None
        } else {
            waits_for_crate(c, job, &pending_pkgs)
        };
        for step in job.steps() {
            if failed {
                items.push(item(job, step, Action::Update, "not attempted".to_string()));
                continue;
            }
            if let Some(why) = crate_wait.as_ref().filter(|_| !step.github()) {
                if !pending_pkgs.contains(&job.pkg.name) {
                    pending_pkgs.push(job.pkg.name.clone());
                }
                items.push(item(job, step, Action::Pending, why.clone()));
                continue;
            }
            if let Some(dep) = waits_on(job, step, &pending_pkgs) {
                let why = format!("waits for {dep}, which is not on its registry yet");
                // Held in turn: whatever depends on this package waits too.
                if !pending_pkgs.contains(&job.pkg.name) {
                    pending_pkgs.push(job.pkg.name.clone());
                }
                items.push(item(job, step, Action::Pending, why));
                continue;
            }
            if waits_for_platforms(job, step, platform_pending) {
                if !pending_pkgs.contains(&job.pkg.name) {
                    pending_pkgs.push(job.pkg.name.clone());
                }
                items.push(item(
                    job,
                    step,
                    Action::Pending,
                    PLATFORMS_PENDING.to_string(),
                ));
                continue;
            }
            let mut warnings = Vec::new();
            match run_step(c, dir, repo, job, step, head, &mut warnings) {
                Ok(Outcome::Done(why)) => {
                    done.push((items.len(), job, step));
                    items.push(item(job, step, Action::Ok, why));
                }
                Ok(Outcome::Pending(why, human)) => {
                    platform_pending |= matches!(step, Step::Platform(_));
                    if !pending_pkgs.contains(&job.pkg.name) {
                        pending_pkgs.push(job.pkg.name.clone());
                    }
                    add_step(&mut humans, human);
                    items.push(item(job, step, Action::Pending, why));
                }
                Err(e) => {
                    failed = true;
                    items.push(item(job, step, Action::Update, format!("failed: {e}")));
                }
            }
            for w in warnings {
                items.push(item_of(job, "crates_token", Action::Unknown, w));
            }
        }
    }
    if failed {
        return report(items, humans, 1);
    }
    let mut all_ok = true;
    for (index, job, step) in done {
        let v = match step {
            Step::Tag | Step::Release => github::verify_step(&c.gh, repo, job, step, head),
            Step::Npm | Step::Crates | Step::Platform(_) => registries::verify(c, job, step),
            Step::Deploy => continue,
        };
        all_ok &= v.ok;
        items[index].verify = Some(v);
    }
    // A held package (waiting on a dependency that is not published yet) is pending even when
    // the held one's own first-publish step is not among the human steps (npm only, crate held).
    let held = items.iter().any(|i| i.action == Action::Pending);
    let code = match (all_ok, humans.is_empty() && !held) {
        (false, _) => 1,
        (true, false) => 2,
        (true, true) => 0,
    };
    report(items, humans, code)
}
