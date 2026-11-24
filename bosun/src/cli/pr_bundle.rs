//! `bosun pr <lead> --bundle N...`: the `Closes` lines of a single-shape bundle. Gated tasks
//! bundle like any task (D-021: a gated PR has no line cap and may hold several tasks; risk-check
//! still asks the owner for its gated paths). With the owner's opt-in
//! (`[approvals] bundle_across_epics = true` on the default branch) a bundle may also take tasks
//! of other single-shape epics: one big PR.

use crate::app::Result;
use crate::app::default_branch;
use crate::cli::plan::epic_shape;
use crate::config::{CONFIG_FILE, Config};
use crate::core::meta::{closing_issues_in, parse_task_meta};
use crate::github::{Github, Issue, Pull};
use crate::output::BosunError;

/// The `Closes #N` lines for the issues bundled with `lead` (`bosun pr <lead> --bundle N...`),
/// each on its own line. Every one must be an open task of the lead's epic, whose shape is
/// single, or (owner opt-in, [`across_epics`]) of another single-shape epic; else exit 1 before
/// anything changes.
pub(crate) fn bundle_lines(gh: &dyn Github, lead: &Issue, bundle: &[u64]) -> Result<String> {
    let mut out = String::new();
    if bundle.is_empty() {
        return Ok(out);
    }
    let lead_meta = parse_task_meta(lead.body_text());
    if lead.state != "open" || lead_meta.is_none() {
        return Err(BosunError::error(
            format!(
                "#{} is closed or not a bosun task: only an open task leads a bundle",
                lead.number
            ),
            "pass only the `bundle` issues `bosun wave plan` listed",
        ));
    }
    let lead_epic = lead_meta.and_then(|m| m.epic);
    let single_epic = |e: Option<u64>| -> Result<bool> {
        Ok(match e {
            Some(e) => gh
                .get_issue(e)?
                .is_some_and(|i| epic_shape(i.body_text()).as_deref() == Some("single")),
            None => false,
        })
    };
    let single = single_epic(lead_epic)?;
    let mut across: Option<bool> = None;
    let refuse = |why: String| {
        BosunError::error(
            why,
            "pass only the `bundle` issues `bosun wave plan` listed",
        )
    };
    if !single {
        return Err(refuse(format!(
            "#{} is not a task of a single-shape epic: only those bundle",
            lead.number
        )));
    }
    for (i, &n) in bundle.iter().enumerate() {
        if n == lead.number || bundle[..i].contains(&n) {
            return Err(refuse(format!("#{n} is listed twice or is the lead")));
        }
        let issue = gh
            .get_issue(n)?
            .filter(|i| !i.pull_request && i.state == "open")
            .ok_or_else(|| refuse(format!("#{n} is not an open issue")))?;
        let meta = parse_task_meta(issue.body_text())
            .ok_or_else(|| refuse(format!("#{n} is not a bosun task")))?;
        if meta.epic != lead_epic {
            if !*across.get_or_insert(across_epics(gh)?) {
                return Err(refuse(format!(
                    "#{n} belongs to another epic (the owner allows one big PR across epics with `[approvals] bundle_across_epics = true` in .bosun/bosun.toml on the default branch)"
                )));
            }
            if !single_epic(meta.epic)? {
                return Err(refuse(format!(
                    "#{n} is not a task of a single-shape epic: only those bundle"
                )));
            }
        }
        out.push_str(&format!("Closes #{n}\n"));
    }
    Ok(out)
}

/// The owner's opt-in to bundles across epics, read from the default branch (a missing or
/// unparsable file is off; an API error is an error).
fn across_epics(gh: &dyn Github) -> Result<bool> {
    let branch = default_branch(gh)?;
    let text = gh
        .file_at(CONFIG_FILE, &format!("refs/heads/{branch}"))?
        .unwrap_or_default();
    Ok(Config::parse(&text, CONFIG_FILE).is_ok_and(|c| c.approvals.bundle_across_epics))
}

/// An existing PR must already close every bundled issue; its body is never edited here (exit 3).
pub(crate) fn require_bundle_closes(pull: &Pull, repo: &str, bundle: &[u64]) -> Result<()> {
    let closed = closing_issues_in(&pull.body, Some(repo));
    if bundle.iter().all(|n| closed.contains(n)) {
        return Ok(());
    }
    Err(BosunError::conflict(
        format!(
            "PR #{} does not close every bundled issue ({})",
            pull.number,
            bundle
                .iter()
                .map(|n| format!("#{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        "add `Closes #N` for each bundled issue to the PR body, then rerun",
    ))
}
