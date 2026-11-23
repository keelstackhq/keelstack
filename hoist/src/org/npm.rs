//! `[npm]` of org.toml (spec §16.2, §16.3). Reads are GET only; npm has no writes. The
//! package reads are anonymous; the trusted publisher read needs `NPM_TOKEN` (§16.5) and is
//! `unknown` without it, on a 401/403 or on an answer hoist cannot parse.

use super::{NoWrite, SectionPlan, TrustRead, item, read_trust, step};
use crate::auth::{Secret, from_env};
use crate::error::HoistError;
use crate::http::{Fetched, Http, Service};
use crate::ops::release::bootstrap::Trust;
use crate::ops::release::bootstrap::trust::trust_command;
use crate::release::registry::npm_name;
use crate::repo::git::GithubRepo;
use crate::report::Action;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedPublisher {
    pub repository: String,
    pub workflow: String,
    pub environment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpmCfg {
    /// Without the `@`.
    pub scope: String,
    #[serde(default)]
    pub packages: Vec<String>,
    pub trusted_publisher: Option<TrustedPublisher>,
}

pub(super) fn validate(cfg: &NpmCfg) -> Result<(), String> {
    let prefix = format!("@{}/", cfg.scope);
    match cfg.packages.iter().find(|p| !p.starts_with(&prefix)) {
        Some(p) => Err(format!("[npm] package `{p}` must be `{prefix}<name>`")),
        None if cfg.scope.is_empty() || cfg.scope.starts_with('@') => {
            Err("[npm] scope must be non-empty and written without `@`".to_string())
        }
        None => Ok(()),
    }
}

/// The anonymous registry client (reads need no token).
pub(super) fn client(verbose: bool) -> Option<Http> {
    Some(Http::new(Service::Npm, Secret::from("")).verbose(verbose))
}

/// `"o/r"` of a trusted publisher as a repo.
pub(super) fn repo_of(t: &TrustedPublisher) -> GithubRepo {
    let (owner, name) = t.repository.split_once('/').unwrap_or((&t.repository, ""));
    GithubRepo {
        owner: owner.into(),
        name: name.into(),
    }
}

/// One config entry of `npm trust list --json` (flat, or with a `claims` object).
fn entry_matches(entry: &Value, t: &TrustedPublisher) -> bool {
    let field = |keys: &[&str]| {
        [Some(entry), entry.get("claims")]
            .into_iter()
            .flatten()
            .find_map(|s| keys.iter().find_map(|k| s.get(*k)))
    };
    let repo_ok = field(&["repository", "repo"])
        .and_then(Value::as_str)
        .is_some_and(|r| r.eq_ignore_ascii_case(&t.repository));
    let want = t
        .workflow
        .strip_prefix(".github/workflows/")
        .unwrap_or(&t.workflow);
    let file_ok = field(&[
        "workflow_ref",
        "file",
        "workflow",
        "workflowFile",
        "workflow_file",
    ])
    .and_then(Value::as_str)
    .is_some_and(|f| f.strip_prefix(".github/workflows/").unwrap_or(f) == want);
    let env_ok = t
        .environment
        .as_deref()
        .is_none_or(|e| field(&["environment", "env"]).and_then(Value::as_str) == Some(e));
    repo_ok && file_ok && env_ok
}

/// Whether the answer (an array, or an object holding the entries or being one) has a match.
fn trusted(v: &Value, t: &TrustedPublisher) -> bool {
    match v {
        Value::Array(a) => a.iter().any(|e| entry_matches(e, t)),
        Value::Object(o) => {
            entry_matches(v, t)
                || o.values()
                    .filter_map(Value::as_array)
                    .flatten()
                    .any(|e| entry_matches(e, t))
        }
        _ => false,
    }
}

/// The step command (`npm trust github <pkg> …`), as `hoist release bootstrap` prints it.
fn command(pkg: &str, t: &TrustedPublisher) -> String {
    match &t.environment {
        Some(env) => trust_command(
            pkg,
            &Trust {
                repo: repo_of(t),
                workflow: t.workflow.clone(),
                env: env.clone(),
                workflows: BTreeMap::new(),
                deadline: String::new(),
            },
        ),
        None => format!(
            "npm trust github {pkg} --file {} --repo {} --allow-publish",
            t.workflow, t.repository
        ),
    }
}

pub(crate) fn read(cfg: &NpmCfg, http: Option<&Http>) -> Result<SectionPlan<NoWrite>, HoistError> {
    let mut p = SectionPlan::default();
    let scope = format!("@{}", cfg.scope);
    let Some(http) = http else {
        return Ok(p);
    };
    // Some(true) present, Some(false) absent, None unreadable.
    let mut exists: Vec<(&String, Option<bool>, String)> = Vec::new();
    for name in &cfg.packages {
        let (found, why) = match http.get_json(&format!("/{}", npm_name(name))) {
            Ok(Fetched::Ok(_)) => (Some(true), String::new()),
            Ok(Fetched::NotFound) => (Some(false), String::new()),
            Ok(Fetched::Forbidden(h)) => (None, format!("cannot read `{name}`: {h}")),
            Err(e) => (None, format!("cannot read `{name}`: {e}")),
        };
        exists.push((name, found, why));
    }
    let any = exists.iter().any(|(_, f, _)| *f == Some(true));
    let all_absent = exists.iter().all(|(_, f, _)| *f == Some(false));
    if any {
        p.items.push(item(
            "npm",
            "scope",
            &scope,
            Action::Ok,
            "a listed package exists",
        ));
    } else if all_absent {
        p.items.push(item(
            "npm",
            "scope",
            &scope,
            Action::Unknown,
            "no listed package exists yet; cannot tell whether the scope exists",
        ));
        p.steps.push(step(
            format!("npm:scope:{scope}"),
            format!(
                "create the npm organization `{}` (hoist verifies it once a listed package exists)",
                cfg.scope
            ),
            "https://www.npmjs.com/org/create",
            "pending",
        ));
    } else {
        p.items.push(item(
            "npm",
            "scope",
            &scope,
            Action::Unknown,
            "cannot read the listed packages",
        ));
    }
    let token = from_env("NPM_TOKEN");
    let trust_http = token.map(|t| Http::with_base_like(http, t));
    for (name, found, why) in &exists {
        match found {
            Some(true) => p
                .items
                .push(item("npm", "package", name, Action::Ok, "published")),
            Some(false) => {
                p.items.push(item(
                    "npm",
                    "package",
                    name,
                    Action::Pending,
                    "not on the registry (404)",
                ));
                p.steps.push(step(
                    format!("npm:package:{name}"),
                    format!("publish `{name}` once with `hoist release bootstrap` (hoist verifies it on the registry)"),
                    &format!("https://www.npmjs.com/package/{name}"),
                    "pending",
                ));
            }
            None => p
                .items
                .push(item("npm", "package", name, Action::Unknown, why)),
        }
        let Some(t) = &cfg.trusted_publisher else {
            continue;
        };
        let unknown =
            |reason: &str| item("npm", "trusted_publisher", name, Action::Unknown, reason);
        if *found != Some(true) {
            p.items
                .push(unknown("the package is not readable on the registry yet"));
            continue;
        }
        let Some(th) = &trust_http else {
            p.items.push(unknown(
                "no npm token (set NPM_TOKEN) to read trusted publishers",
            ));
            continue;
        };
        match read_trust(th, &format!("/-/package/{}/trust", npm_name(name)))? {
            TrustRead::Unknown(why) => p.items.push(unknown(&why)),
            TrustRead::Got(v) if trusted(&v, t) => p.items.push(item(
                "npm",
                "trusted_publisher",
                name,
                Action::Ok,
                format!("trusts {} ({})", t.repository, t.workflow),
            )),
            TrustRead::Got(_) | TrustRead::Absent => {
                p.items.push(item(
                    "npm",
                    "trusted_publisher",
                    name,
                    Action::Pending,
                    format!("no trusted publisher for {} ({})", t.repository, t.workflow),
                ));
                p.steps.push(step(
                    format!("npm:trusted_publisher:{name}"),
                    format!(
                        "run `{}` (hoist verifies it through the registry trust read)",
                        command(name, t)
                    ),
                    &format!("https://www.npmjs.com/package/{name}/access"),
                    "pending",
                ));
            }
        }
    }
    Ok(p)
}
