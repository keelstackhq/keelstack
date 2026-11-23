//! `[crates]` of org.toml (spec §16.2, §16.3). Reads are GET only; crates has no writes. Owners
//! read anonymously; the trusted publisher read needs `CARGO_REGISTRY_TOKEN` (§16.5, sent as
//! the bare token) and is `unknown` without it or on a 401/403 (§16.6).

use super::npm::TrustedPublisher;
use super::{NoWrite, SectionPlan, TrustRead, item, read_trust, step};
use crate::auth::{Secret, from_env};
use crate::error::HoistError;
use crate::http::{Fetched, Http, Service, seg};
use crate::report::Action;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CratesCfg {
    #[serde(default)]
    pub crates: Vec<String>,
    /// Logins or `github:<org>:<team>`.
    #[serde(default)]
    pub owners: Vec<String>,
    pub trusted_publisher: Option<TrustedPublisher>,
}

/// The anonymous crates.io client (owner reads need no token).
pub(super) fn client(verbose: bool) -> Option<Http> {
    Some(Http::new(Service::Crates, Secret::from("")).verbose(verbose))
}

fn config_matches(c: &Value, t: &TrustedPublisher) -> bool {
    let s = |k: &str| c.get(k).and_then(Value::as_str);
    let (owner, name) = t.repository.split_once('/').unwrap_or((&t.repository, ""));
    s("repository_owner").is_some_and(|o| o.eq_ignore_ascii_case(owner))
        && s("repository_name").is_some_and(|n| n.eq_ignore_ascii_case(name))
        && s("workflow_filename").is_some_and(|f| f == t.workflow)
        && t.environment
            .as_deref()
            .is_none_or(|e| s("environment") == Some(e))
}

pub(crate) fn read(
    cfg: &CratesCfg,
    http: Option<&Http>,
) -> Result<SectionPlan<NoWrite>, HoistError> {
    let mut p = SectionPlan::default();
    let Some(http) = http else {
        return Ok(p);
    };
    let trust_http = from_env("CARGO_REGISTRY_TOKEN").map(|t| Http::with_base_like(http, t));
    for c in &cfg.crates {
        let settings = format!("https://crates.io/crates/{c}/settings");
        let owners = match http.get_json(&format!("/api/v1/crates/{}/owners", seg(c))) {
            Ok(Fetched::Ok(v)) => Ok(["users", "teams"]
                .iter()
                .filter_map(|k| v.get(*k).and_then(Value::as_array))
                .flatten()
                .filter_map(|u| u.get("login").and_then(Value::as_str))
                .map(str::to_lowercase)
                .collect::<Vec<_>>()),
            Ok(Fetched::NotFound) => Err(true),
            Ok(Fetched::Forbidden(_)) | Err(_) => Err(false),
        };
        match &owners {
            Ok(logins) => {
                for o in &cfg.owners {
                    let key = format!("{c}/{o}");
                    if logins.contains(&o.to_lowercase()) {
                        p.items
                            .push(item("crates", "crate_owner", &key, Action::Ok, "owner"));
                    } else {
                        p.items.push(item(
                            "crates",
                            "crate_owner",
                            &key,
                            Action::Pending,
                            "not an owner",
                        ));
                        p.steps.push(step(
                            format!("crates:crate_owner:{key}"),
                            format!(
                                "run `cargo owner --add {o} {c}` (hoist verifies it through the crate's owners)"
                            ),
                            &settings,
                            "pending",
                        ));
                    }
                }
            }
            Err(missing) => {
                let (action, reason) = if *missing {
                    (Action::Pending, "the crate is not on crates.io (404)")
                } else {
                    (Action::Unknown, "cannot read the crate's owners")
                };
                for o in &cfg.owners {
                    p.items.push(item(
                        "crates",
                        "crate_owner",
                        &format!("{c}/{o}"),
                        action,
                        reason,
                    ));
                }
                if *missing {
                    p.steps.push(step(
                        format!("crates:crate:{c}"),
                        format!("publish `{c}` once with `hoist release bootstrap` (hoist verifies it on crates.io)"),
                        &format!("https://crates.io/crates/{c}"),
                        "pending",
                    ));
                }
            }
        }
        let Some(t) = &cfg.trusted_publisher else {
            continue;
        };
        let unknown = |why: &str| item("crates", "trusted_publisher", c, Action::Unknown, why);
        if owners.is_err() {
            p.items
                .push(unknown("the crate is not readable on crates.io yet"));
            continue;
        }
        let Some(th) = &trust_http else {
            p.items.push(unknown(
                "no crates.io token (set CARGO_REGISTRY_TOKEN) to read trusted publishers",
            ));
            continue;
        };
        match read_trust(
            th,
            &format!("/api/v1/trusted_publishing/github_configs?crate={}", seg(c)),
        )? {
            TrustRead::Unknown(why) => p.items.push(unknown(&why)),
            TrustRead::Got(v)
                if v.get("github_configs")
                    .and_then(Value::as_array)
                    .is_some_and(|l| l.iter().any(|g| config_matches(g, t))) =>
            {
                p.items.push(item(
                    "crates",
                    "trusted_publisher",
                    c,
                    Action::Ok,
                    format!("trusts {} ({})", t.repository, t.workflow),
                ));
            }
            TrustRead::Got(_) | TrustRead::Absent => {
                p.items.push(item(
                    "crates",
                    "trusted_publisher",
                    c,
                    Action::Pending,
                    format!("no trusted publisher for {} ({})", t.repository, t.workflow),
                ));
                p.steps.push(step(
                    format!("crates:trusted_publisher:{c}"),
                    format!(
                        "add a trusted publisher to crate {c}: repository {}, workflow {}{} (hoist verifies it through the trust read)",
                        t.repository,
                        t.workflow,
                        t.environment.as_ref().map_or(String::new(), |e| format!(", environment {e}"))
                    ),
                    &settings,
                    "pending",
                ));
            }
        }
    }
    Ok(p)
}
