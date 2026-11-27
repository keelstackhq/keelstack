//! Behaviour checks for `hoist check` (spec §7). Offline against the working tree, or
//! anonymous GETs of public URLs; no token is ever sent to a site host.

use crate::cloudflare::PagesProject;
use crate::desired::Desired;
use crate::http::probe;
use crate::repo::workflows::Workflows;
use crate::report::{Action, Item, Verify};

/// Env override (tests only) that replaces `https://<subdomain>` with `<base>`.
const PAGES_BASE_ENV: &str = "HOIST_PAGES_BASE";

/// Commit statuses bosun workflows post; a required check of one of these names is not a job.
const BOSUN_STATUSES: [&str; 3] = ["main-open", "review-verdict", "risk-check"];

/// The id of the GitHub Actions App, the source of every workflow job's check.
const GITHUB_ACTIONS_APP: u64 = 15368;

/// One `github/required_check` item per required check: it must be a job id or job `name` in a
/// workflow under `.github/workflows/`.
///
/// Known limit: a commit status verifies when any one job has `statuses: write`; hoist does not
/// know which job posts which status, so one such job lets every listed status verify, even when
/// that job posts only some of them.
///
/// A check the live ruleset pins to an App other than GitHub Actions (`integration_id` not
/// 15368) is posted by that App, not by a workflow job: it is satisfied without one (spec §6.1).
/// `pins` are the live `(context, integration_id)` pairs of the default-branch ruleset.
pub fn verify_required_checks(d: &Desired, w: &Workflows, pins: &[(String, u64)]) -> Vec<Item> {
    d.github
        .ruleset
        .required_checks
        .iter()
        .map(|check| {
            let found = w.jobs.iter().any(|j| j.value == check.value);
            let status = BOSUN_STATUSES.contains(&check.value.as_str())
                || d.github.statuses.contains(&check.value);
            // A job without `permissions` gets the repo default, which apply sets.
            let mut writers = w.status_writers.clone();
            if d.github.actions_permissions.value == "write" {
                writers.extend(w.default_permission_jobs.iter().cloned());
            }
            let kept = check.source == crate::adopt::SOURCE;
            let app = pins
                .iter()
                .find(|(c, id)| *c == check.value && *id != GITHUB_ACTIONS_APP)
                .map(|(_, id)| *id);
            // Pinned in hoist.toml to an App other than GitHub Actions: no workflow job posts it.
            // Without a live pin to compare (ruleset unreadable or unpinned) it is `unknown`, and
            // the ruleset item reports any real mismatch.
            let wanted_app = d
                .github
                .ruleset
                .check_pins
                .iter()
                .find(|p| p.context == check.value && p.id != Some(GITHUB_ACTIONS_APP));
            let mut action = Action::Ok;
            let (reason, ok) = if let Some(id) = app {
                (format!("posted by app {id}"), true)
            } else if let Some(pin) = wanted_app {
                action = Action::Unknown;
                let who = match &pin.app {
                    crate::config::CheckApp::Slug(s) => format!("app `{s}`"),
                    crate::config::CheckApp::Id(i) => format!("app id {i}"),
                };
                (
                    format!("pinned to {who}, posted by it, not a workflow job; not verified live"),
                    true,
                )
            } else if kept {
                (
                    "kept from the live ruleset (--adopt); not a check hoist names".to_string(),
                    true,
                )
            } else if status && !found && !writers.is_empty() {
                (
                    format!(
                        "a commit status (built-in bosun set or `[github] statuses`); \
                         `statuses: write` in {}",
                        writers.join(", ")
                    ),
                    true,
                )
            } else if status && !found {
                (
                    format!(
                        "required check `{}` is a commit status (built-in bosun set or \
                         `[github] statuses`), but no workflow job has `statuses: write` to \
                         post it",
                        check.value
                    ),
                    false,
                )
            } else if found {
                ("a job with this name exists in the workflows".to_string(), true)
            } else {
                (
                    format!(
                        "required check `{}` is not the name of any job in `.github/workflows/*.yml`",
                        check.value
                    ),
                    false,
                )
            };
            Item {
                area: "github".to_string(),
                kind: "required_check".to_string(),
                key: check.value.clone(),
                action,
                reason: reason.clone(),
                source: check.source.clone(),
                verify: Some(Verify { ok, detail: reason }),
            }
        })
        .collect()
}

/// 2xx is ok. Only things that exist are probed, so a 404 means no deployment answers yet.
fn verdict(url: &str, result: Result<u16, String>) -> Verify {
    match result {
        Ok(200..=299) => Verify {
            ok: true,
            detail: format!("GET {url} answered 2xx"),
        },
        Ok(404) => Verify {
            ok: false,
            detail: "no deployment answers yet (404)".to_string(),
        },
        Ok(status) => Verify {
            ok: false,
            detail: format!("GET {url} answered {status}"),
        },
        Err(why) => Verify {
            ok: false,
            detail: format!("GET {url} failed: {why}"),
        },
    }
}

/// `GET https://<subdomain>/` (base from `HOIST_PAGES_BASE` when set): 2xx is ok. A 404 on an
/// existing project means the first deploy has not happened yet, which is still a failure.
pub fn verify_pages(p: &PagesProject) -> Verify {
    let base = std::env::var(PAGES_BASE_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim_end_matches('/').to_string())
        .unwrap_or_else(|| format!("https://{}", p.subdomain));
    let url = format!("{base}/");
    verdict(&url, probe(&url))
}

/// HTTPS GET `https://<host>/`: 2xx with a certificate the platform roots accept (rustls
/// validation; a bad certificate is a transport error and fails the check). Only attached
/// domains are probed, so a 404 is "no deployment answers yet", a failure (spec §7.4).
pub fn verify_domain(host: &str) -> Verify {
    let url = format!("https://{host}/");
    verdict(&url, probe(&url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_classifies_statuses() {
        assert!(verdict("u", Ok(204)).ok);
        assert!(!verdict("u", Ok(500)).ok);
        assert_eq!(
            verdict("u", Ok(404)).detail,
            "no deployment answers yet (404)"
        );
        assert!(!verdict("u", Ok(404)).ok);
        assert!(!verdict("u", Err("tls".into())).ok);
    }
}
