//! `apply` execution: the writes in order, and the reports of a run that stopped, was
//! declined or could not be re-read after writing.

use super::r2_sink::R2Run;
use super::{find, not_read, writes_cloudflare};
use crate::change::Change;
use crate::cloudflare_write::execute_cloudflare_with;
use crate::desired::Desired;
use crate::error::HoistError;
use crate::github_write::{DEPLOY_KEYS_POLICY, execute_github};
use crate::http::Http;
use crate::ops::check::exit_code;
use crate::report::{Action, HumanStep, Item, Report, Verify};
use std::cmp::Ordering;

/// What a complete run of writes leaves for the report.
pub(super) struct Written {
    /// Human steps the writers returned (a domain whose zone is not on the account).
    pub steps: Vec<HumanStep>,
    /// `(kind, key)` of Pages projects and domains created now: no deployment yet (R17).
    pub created: Vec<(String, String)>,
    /// The ruleset write the GitHub plan refused (Free, private repo).
    pub plan_limited: bool,
    /// The release environment was written without its reviewer (plan refused one).
    pub reviewer_limited: bool,
}

/// Run every change in order. The first failed write stops the run: `Err` is its report
/// (applied, `failed: <error>`, not attempted; exit 1).
pub(super) fn write_all(
    gh: Option<&Http>,
    cf: Option<&Http>,
    d: &Desired,
    changes: &[Change],
    plan: &Report,
    groups: Option<&[String]>,
    r2: Option<&R2Run>,
) -> Result<Written, Report> {
    let mut steps = Vec::new();
    let mut created = Vec::new();
    let mut plan_limited = false;
    let mut reviewer_limited = false;
    let mut policy: Option<HumanStep> = None;
    for (index, change) in changes.iter().enumerate() {
        // Deploy keys are not allowed: the tag ruleset would bypass a key that cannot exist.
        if policy.is_some() && matches!(change, Change::TagRuleset { .. }) {
            continue;
        }
        match execute(gh, cf, d, change, groups, r2) {
            // Not a stop: the other writes still apply, and the report names the gap.
            Err(HoistError::PlanLimited(_))
                if matches!(change, Change::Ruleset { .. } | Change::TagRuleset { .. }) =>
            {
                plan_limited = true;
            }
            Err(HoistError::PlanLimited(_)) if matches!(change, Change::Environment { .. }) => {
                reviewer_limited = true;
            }
            Ok(step) => {
                if let Some(s) = step.as_ref().filter(|s| s.id == DEPLOY_KEYS_POLICY) {
                    policy = Some(s.clone());
                }
                match (&step, change) {
                    (None, Change::PagesProject { name, exists, .. }) if !exists => {
                        created.push(("pages_project".to_string(), name.clone()))
                    }
                    (None, Change::Domain { host, .. }) => {
                        created.push(("custom_domain".to_string(), host.clone()))
                    }
                    _ => {}
                }
                steps.extend(step);
            }
            Err(err) => {
                let status = |i: usize| match i.cmp(&index) {
                    Ordering::Less => (true, "applied".to_string()),
                    Ordering::Equal => (false, format!("failed: {err}")),
                    Ordering::Greater => (false, "not attempted".to_string()),
                };
                steps.extend(r2.map(R2Run::take_left_over).unwrap_or_default());
                let mut report = stopped(changes, plan, status, steps);
                if let Some(step) = &policy {
                    // Only the items a change stood for: a planned `drift` or `ok` item has
                    // no change and stays as it was.
                    let kinds: Vec<&str> = changes
                        .iter()
                        .filter_map(|c| match c {
                            Change::TagDeployKey { .. } => Some("tag_deploy_key"),
                            Change::TagRuleset { .. } => Some("tag_ruleset"),
                            _ => None,
                        })
                        .collect();
                    mark_deploy_keys_pending(&mut report, step, |i| {
                        kinds.contains(&i.kind.as_str())
                    });
                    report.exit_code = 1;
                }
                if plan_limited {
                    mark_plan_limited(&mut report, d);
                }
                if reviewer_limited {
                    mark_reviewer_limited(&mut report);
                }
                return Err(report);
            }
        }
    }
    Ok(Written {
        steps,
        created,
        plan_limited,
        reviewer_limited,
    })
}

/// The ruleset write the plan refused: its item is `pending` naming what is not enforced, with
/// the human step `plan-limit:rulesets` (a plan limit is no drift hoist can fix, but nothing
/// protects the branch: exit 2, never a silent 0), never `applied` or a re-read's `create`.
pub(super) fn mark_plan_limited(report: &mut Report, d: &Desired) {
    for item in report.items.iter_mut().filter(|i| i.kind == "ruleset") {
        item.action = Action::Pending;
        item.reason = crate::diff::free_plan_reason(d);
        item.verify = None;
    }
    if let Some(t) = &d.github.tag_ruleset {
        for item in report.items.iter_mut().filter(|i| i.kind == "tag_ruleset") {
            item.action = Action::Pending;
            item.reason = crate::diff::free_plan_tag_reason(&t.refs());
            item.verify = None;
        }
    }
    crate::diff::add_plan_limit_step(&mut report.human_steps, d);
    // The refused item is a human step: 2 unless another item failed (1).
    report.exit_code = crate::ops::check::exit_code(&report.items);
}

/// The release environment written without a reviewer: `drift` naming the missing reviewer.
pub(super) fn mark_reviewer_limited(report: &mut Report) {
    for item in report
        .items
        .iter_mut()
        .filter(|i| i.area == "release" && i.kind == "environment")
    {
        item.action = Action::Drift;
        item.reason = "required reviewer missing: the GitHub plan does not allow required \
                       reviewers here; add the release approver by hand (Settings, Environments)"
            .to_string();
        item.verify = None;
    }
    report.exit_code = 1;
}

/// The release environment written without a reviewer because the token's user is a bot:
/// `pending` with the human step `release-reviewer`, so the exit code is 2.
pub(super) fn mark_reviewer_pending(report: &mut Report, d: &Desired) {
    let title = "add a required reviewer to the `release` environment (the token's user is a \
                 bot, so none was set); or set [release] reviewers in hoist.toml and apply again"
        .to_string();
    for item in report
        .items
        .iter_mut()
        .filter(|i| i.area == "release" && i.kind == "environment")
    {
        item.action = Action::Pending;
        item.reason = title.clone();
        item.verify = None;
    }
    let step = HumanStep {
        id: "release-reviewer".to_string(),
        title,
        url: format!(
            "https://github.com/{}/{}/settings/environments",
            d.repo.owner, d.repo.name
        ),
        status: "pending".to_string(),
    };
    if !report.human_steps.iter().any(|s| s.id == step.id) {
        report.human_steps.push(step);
    }
    report.exit_code = match exit_code(&report.items) {
        0 if !report.human_steps.is_empty() => 2,
        code => code,
    };
}

/// All writes succeeded but the re-read or check failed: the applied changes plus a
/// `post_apply_check` item with the error, exit 1.
pub(super) fn late_error(
    changes: &[Change],
    plan: &Report,
    steps: Vec<HumanStep>,
    err: &HoistError,
) -> Report {
    let mut report = stopped(changes, plan, |_| (true, "applied".to_string()), steps);
    report.items.push(Item {
        area: "apply".to_string(),
        kind: "post_apply_check".to_string(),
        key: "re-read".to_string(),
        action: Action::Unknown,
        reason: format!("re-read after the writes failed: {err}; run `hoist check` to verify"),
        source: "default".to_string(),
        verify: Some(Verify {
            ok: false,
            detail: err.to_string(),
        }),
    });
    report
}

/// The report of a declined prompt: nothing written, exit 1.
pub(super) fn declined(changes: &[Change], plan: &Report) -> Report {
    stopped(
        changes,
        plan,
        |_| (false, "not applied: declined".to_string()),
        Vec::new(),
    )
}

fn execute(
    gh: Option<&Http>,
    cf: Option<&Http>,
    d: &Desired,
    change: &Change,
    groups: Option<&[String]>,
    r2: Option<&R2Run>,
) -> Result<Option<HumanStep>, HoistError> {
    if let Change::Dns(w) = change {
        let cf = cf.ok_or_else(|| not_read("Cloudflare"))?;
        return crate::dns::execute(cf, w).map(|()| None);
    }
    if let Change::R2(c) = change {
        let cf = cf.ok_or_else(|| not_read("Cloudflare"))?;
        let run = r2.ok_or_else(|| HoistError::Usage("R2 writes were not prepared".into()))?;
        return run.execute(cf, d, c).map(|()| None);
    }
    if writes_cloudflare(change) {
        let cf = cf.ok_or_else(|| not_read("Cloudflare"))?;
        execute_cloudflare_with(cf, gh, d, change, groups)
    } else {
        execute_github(gh.ok_or_else(|| not_read("GitHub"))?, d, change)
    }
}

/// The report items a change stands for: one per patched field for repo settings.
fn keys(change: &Change) -> Vec<(&'static str, &'static str, String)> {
    match change {
        Change::RepoSettings { patch } => patch
            .keys()
            .map(|k| ("github", "repo_setting", k.clone()))
            .collect(),
        _ => vec![change.key()],
    }
}

/// The report of a run that stopped (a failed write, a declined prompt) or could not be
/// verified: one item per change with `status(index)` = (applied, reason), then the plan's
/// items that no change stands for (pending, unknown, ok). Exit 1.
fn stopped(
    changes: &[Change],
    plan: &Report,
    status: impl Fn(usize) -> (bool, String),
    steps: Vec<HumanStep>,
) -> Report {
    let mut items = Vec::new();
    let mut covered = Vec::new();
    for (index, change) in changes.iter().enumerate() {
        let (done, reason) = status(index);
        for (area, kind, key) in keys(change) {
            let planned = find(&plan.items, area, kind, &key);
            let action = match (done, planned) {
                (true, _) => Action::Ok,
                (false, Some(p)) => p.action,
                (false, None) => Action::Update,
            };
            covered.push((area, kind, key.clone()));
            items.push(Item {
                area: area.to_string(),
                kind: kind.to_string(),
                key,
                action,
                reason: reason.clone(),
                source: planned.map_or_else(|| "default".to_string(), |p| p.source.clone()),
                verify: None,
            });
        }
    }
    // Create/update items all pair with a change (R16); the rest is kept as planned.
    items.extend(
        plan.items
            .iter()
            .filter(|i| !matches!(i.action, Action::Create | Action::Update))
            .filter(|i| {
                !covered
                    .iter()
                    .any(|(a, k, key)| *a == i.area && *k == i.kind && *key == i.key)
            })
            .cloned(),
    );
    let mut report = Report {
        command: "apply".to_string(),
        items,
        human_steps: plan.human_steps.clone(),
        exit_code: 1,
        text: None,
        hint: None,
    };
    for step in steps {
        if !report.human_steps.iter().any(|s| s.id == step.id) {
            report.human_steps.push(step);
        }
    }
    report
}

/// Deploy keys are not allowed: the tag deploy key and the tag ruleset were not written, so
/// their items are `pending` (not drift, not applied) with the step's title as reason.
fn mark_deploy_keys_pending(
    report: &mut Report,
    step: &HumanStep,
    skipped: impl Fn(&Item) -> bool,
) {
    for item in report
        .items
        .iter_mut()
        .filter(|i| (i.kind == "tag_deploy_key" || i.kind == "tag_ruleset") && skipped(i))
    {
        item.action = Action::Pending;
        item.reason = step.title.clone();
        item.verify = None;
    }
}

/// Add the human steps the writers returned (a domain whose zone is not on the account): the
/// matching item becomes `pending`, so the exit code is 2 when nothing else failed.
pub(super) fn add_steps(report: &mut Report, steps: Vec<HumanStep>) {
    for step in steps {
        if step.id == DEPLOY_KEYS_POLICY {
            // Only what the skipped writes would have changed; `drift` and `ok` stay.
            mark_deploy_keys_pending(report, &step, |i| {
                matches!(i.action, Action::Create | Action::Update)
            });
        }
        if let Some(host) = step.id.strip_prefix("domain:") {
            for item in report.items.iter_mut().filter(|i| {
                i.kind == "custom_domain" && i.key == host && i.action == Action::Create
            }) {
                item.action = Action::Pending;
                item.reason = step.title.clone();
            }
        }
        if !report.human_steps.iter().any(|s| s.id == step.id) {
            report.human_steps.push(step);
        }
    }
    report.exit_code = match exit_code(&report.items) {
        0 if !report.human_steps.is_empty() => 2,
        code => code,
    };
}
