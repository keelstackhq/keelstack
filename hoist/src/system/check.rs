//! `hoist system check` (spec §17.4): offline checks of system.toml against the workflows,
//! `hoist.toml` and `secrets.toml`; `--live` adds the per-provider probes (§17.5).

use super::live::{self, LiveResult};
use super::{Credential, Expiry, FILE, Status, SystemFile, add_duration, parse_expiry, source_of};
use crate::error::HoistError;
use crate::repo::workflows::read_workflows;
use crate::report::{Action, HumanStep, Item, Report};
use crate::secrets::manifest::Manifest;
use crate::timeutil::{civil_from_days, now_secs, parse_date};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Days before expiry at which rotation is a human step (as for the Cloudflare deploy token).
const ROTATE_WITHIN_DAYS: i64 = 30;

/// A secret name the repo uses, and where (`path:line` or file).
pub(crate) struct Uses {
    /// secret -> first source.
    pub names: BTreeMap<String, String>,
    /// secret -> `file:job` of every job reading it.
    pub jobs: BTreeMap<String, BTreeSet<String>>,
    /// secret -> environment of a job reading it (first seen).
    pub envs: BTreeMap<String, Option<String>>,
    /// Names only `secrets.toml` or `optional_secrets` list (no job reads them).
    pub listed: BTreeSet<String>,
}

pub(crate) fn path_of(dir: &Path, file: Option<&Path>) -> PathBuf {
    file.map_or_else(
        || dir.join(FILE),
        |f| {
            if f.is_absolute() {
                f.to_path_buf()
            } else {
                dir.join(f)
            }
        },
    )
}

/// What the repo says about secret names: workflows, `[github] optional_secrets`, secrets.toml.
pub(crate) fn repo_uses(dir: &Path) -> Result<Uses, HoistError> {
    let wf = read_workflows(dir)?;
    let mut u = Uses {
        names: BTreeMap::new(),
        jobs: BTreeMap::new(),
        envs: BTreeMap::new(),
        listed: BTreeSet::new(),
    };
    for s in wf.secrets.iter().filter(|s| s.value != "GITHUB_TOKEN") {
        u.names
            .entry(s.value.clone())
            .or_insert_with(|| s.source.clone());
    }
    for s in wf.secret_uses.iter().filter(|s| s.secret != "GITHUB_TOKEN") {
        u.jobs
            .entry(s.secret.clone())
            .or_default()
            .insert(s.job.clone());
        u.envs
            .entry(s.secret.clone())
            .or_insert_with(|| s.environment.clone());
    }
    for name in optional_secrets(dir)? {
        u.names
            .entry(name.clone())
            .or_insert_with(|| "hoist.toml:github.optional_secrets".into());
        u.listed.insert(name);
    }
    if let Ok(text) = std::fs::read_to_string(dir.join("secrets.toml")) {
        let m = Manifest::parse(&text, "secrets.toml")?;
        for (env, spec) in &m.env {
            for var in spec.vars.keys() {
                u.names
                    .entry(var.clone())
                    .or_insert_with(|| format!("secrets.toml:env.{env}"));
                u.listed.insert(var.clone());
            }
        }
    }
    Ok(u)
}

/// `[github] optional_secrets` of the root `hoist.toml` (read raw: only this key matters here).
pub(crate) fn optional_secrets(dir: &Path) -> Result<Vec<String>, HoistError> {
    let Ok(text) = std::fs::read_to_string(dir.join("hoist.toml")) else {
        return Ok(Vec::new());
    };
    let v: toml::Value = toml::from_str(&text).map_err(|e| HoistError::Config {
        file: "hoist.toml".into(),
        message: e.to_string(),
    })?;
    Ok(v.get("github")
        .and_then(|g| g.get("optional_secrets"))
        .and_then(|o| o.as_array())
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_uppercase))
        .collect())
}

fn item(kind: &str, key: &str, action: Action, reason: String, source: String) -> Item {
    Item {
        area: "system".into(),
        kind: kind.into(),
        key: key.into(),
        action,
        reason,
        source,
        verify: None,
    }
}

fn day_str(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

fn todo(v: Option<&str>) -> bool {
    v.is_none_or(|s| s.trim().is_empty() || s.trim() == "TODO")
}

pub fn system_check(
    dir: &Path,
    file: Option<&Path>,
    live_on: bool,
    verbose: bool,
) -> Result<Report, HoistError> {
    let path = path_of(dir, file);
    let sys = SystemFile::load(&path)?;
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let uses = repo_uses(dir)?;
    let today = now_secs().div_euclid(86_400);
    let shown = file.map_or_else(|| FILE.to_string(), |f| f.display().to_string());
    let ctx = live::Ctx {
        dir,
        sys: &sys,
        today,
        verbose,
    };
    let probes = live_on.then(|| live::probes(&ctx));
    let (items, human_steps) = run_checks(&sys, &text, &shown, &uses, today, probes);
    let exit_code = crate::ops::check::exit_code(&items);
    Ok(Report {
        command: "system check".into(),
        items,
        human_steps,
        exit_code,
        text: None,
        hint: None,
    })
}

pub(crate) fn run_checks(
    sys: &SystemFile,
    text: &str,
    file: &str,
    uses: &Uses,
    today: i64,
    probes: Option<Vec<Box<dyn live::LiveProbe + '_>>>,
) -> (Vec<Item>, Vec<HumanStep>) {
    let mut items = Vec::new();
    let mut steps: Vec<HumanStep> = Vec::new();
    let declared: BTreeSet<&str> = sys.credentials.iter().flat_map(Credential::keys).collect();
    for (name, source) in &uses.names {
        if !declared.contains(name.as_str()) {
            items.push(item(
                "undeclared",
                name,
                Action::Drift,
                "secret used but no [[credential]] storage key names it".into(),
                source.clone(),
            ));
        }
    }
    for c in &sys.credentials {
        let src = source_of(text, file, &c.id);
        let before = items.len();
        let keys = c.keys();
        let actual: BTreeSet<&str> = keys
            .iter()
            .filter_map(|k| uses.jobs.get(*k))
            .flatten()
            .map(String::as_str)
            .collect();
        if c.status != Status::Active {
            for j in &actual {
                items.push(item(
                    "consumer",
                    &c.id,
                    Action::Drift,
                    format!("status is {:?} but {j} still reads it", c.status).to_lowercase(),
                    src.clone(),
                ));
            }
            continue;
        }
        let listed: BTreeSet<String> = c.consumers.iter().filter_map(|u| u.job_ref()).collect();
        let runtime = c.consumers.iter().any(|u| u.runtime.is_some());
        let named = keys.iter().any(|k| uses.listed.contains(*k));
        if actual.is_empty() && !runtime && !named {
            items.push(item(
                "unused",
                &c.id,
                Action::Drift,
                "no workflow job reads it and no runtime consumer is listed".into(),
                src.clone(),
            ));
        }
        for j in listed.iter().filter(|j| !actual.contains(j.as_str())) {
            items.push(item(
                "consumer",
                &c.id,
                Action::Drift,
                format!("listed consumer {j} reads none of its keys"),
                src.clone(),
            ));
        }
        for j in actual.iter().filter(|j| !listed.contains(**j)) {
            items.push(item(
                "consumer",
                &c.id,
                Action::Drift,
                format!("{j} reads it but is not in consumers"),
                src.clone(),
            ));
        }
        let mut missing = Vec::new();
        if todo(Some(&c.purpose)) {
            missing.push("purpose");
        }
        if todo(c.owner.as_deref()) {
            missing.push("owner");
        }
        // `role = "config"` marks a non-secret value kept as a secret (an account id): no dates.
        let config = c.role == "config";
        if c.created_at.is_none() && !config {
            missing.push("created_at");
        }
        if c.expires_at.is_none() && !config {
            missing.push("expires_at");
        }
        if !missing.is_empty() {
            items.push(item(
                "metadata",
                &c.id,
                Action::Drift,
                format!("missing {}", missing.join(", ")),
                src.clone(),
            ));
        }
        let created = c.created_at.as_deref().and_then(parse_date);
        let expiry = c.expires_at.as_deref().and_then(parse_expiry);
        let mut rotate = None;
        if let Some(Expiry::On(at)) = expiry {
            let days = at - today;
            if days < 0 {
                items.push(item(
                    "expiry",
                    &c.id,
                    Action::Drift,
                    format!("expired {} days ago", -days),
                    src.clone(),
                ));
            } else if days <= ROTATE_WITHIN_DAYS {
                items.push(item(
                    "expiry",
                    &c.id,
                    Action::Pending,
                    format!("expires in {days} days"),
                    src.clone(),
                ));
                rotate = Some(format!("expires {}", day_str(at)));
            }
        }
        if let Some(ttl) = c.max_ttl.as_deref() {
            let limit = created.and_then(|d| add_duration(d, ttl));
            match (expiry, limit) {
                (Some(Expiry::Never), _) => items.push(item(
                    "max_ttl",
                    &c.id,
                    Action::Drift,
                    format!("expires_at is never but max_ttl is {ttl}"),
                    src.clone(),
                )),
                (Some(Expiry::On(at)), Some(limit)) if at > limit => items.push(item(
                    "max_ttl",
                    &c.id,
                    Action::Drift,
                    format!(
                        "expires {} days after created_at + max_ttl {ttl} ({})",
                        at - limit,
                        day_str(limit)
                    ),
                    src.clone(),
                )),
                _ => {}
            }
        }
        if let (Some(every), Some(created)) = (c.rotate_every.as_deref(), created) {
            let due = add_duration(created, every).unwrap_or(created);
            if due <= today {
                items.push(item(
                    "rotation",
                    &c.id,
                    Action::Pending,
                    format!("rotation due since {} (every {every})", day_str(due)),
                    src.clone(),
                ));
                rotate.get_or_insert_with(|| format!("due since {}", day_str(due)));
            }
        }
        if let Some(why) = keyless(sys, c) {
            items.push(item("keyless", &c.id, Action::Drift, why, src.clone()));
        }
        if let Some(when) = rotate {
            steps.push(rotate_step(sys, c, &when));
        }
        if items.len() == before {
            items.push(item(
                "credential",
                &c.id,
                Action::Ok,
                format!("{} {} for {}", c.provider, c.kind.as_str(), c.role),
                src.clone(),
            ));
        }
        if let Some(p) = &probes {
            let (action, reason) = match live::probe(p, c) {
                LiveResult::Ok(r) => (Action::Ok, r),
                LiveResult::Drift(r) => (Action::Drift, r),
                LiveResult::Unknown(r) => (Action::Unknown, r),
            };
            items.push(item("live", &c.id, action, reason, src.clone()));
        }
    }
    for s in probes.iter().flatten().flat_map(|p| p.sweep()) {
        let dup = s.kind == "undeclared"
            && items
                .iter()
                .any(|i| i.kind == "undeclared" && i.key == s.key);
        if !dup {
            items.push(live_item(s));
        }
    }
    (items, steps)
}

fn live_item(s: live::Sweep) -> Item {
    let (action, reason) = match s.result {
        LiveResult::Ok(r) => (Action::Ok, r),
        LiveResult::Drift(r) => (Action::Drift, r),
        LiveResult::Unknown(r) => (Action::Unknown, r),
    };
    item(s.kind, &s.key, action, reason, s.source)
}

/// Why a long-lived credential should be keyless, or `None`.
fn keyless(sys: &SystemFile, c: &Credential) -> Option<String> {
    use super::CredKind::*;
    if let Some(t) = sys
        .trusts
        .iter()
        .find(|t| t.provider == c.provider && t.replaces.as_deref() == Some(c.role.as_str()))
    {
        return Some(format!(
            "role {} is replaced by {} trust `{}`; revoke this credential",
            c.role,
            t.kind.as_str(),
            t.subject
        ));
    }
    if c.keyless_exempt
        .as_deref()
        .is_some_and(|r| !r.trim().is_empty())
    {
        return None;
    }
    let capability = sys
        .provider(&c.provider)
        .map_or("", |p| p.capability.as_str());
    let registry =
        matches!(c.provider.as_str(), "npm" | "crates") || capability.starts_with("registry.");
    match c.kind {
        _ if registry && c.role == "publish" && c.kind != SshKey => Some(
            "long-lived publish token: use OIDC trusted publishing (or set keyless_exempt)".into(),
        ),
        ApiToken if c.provider == "github" => Some(
            "personal access token: use a GitHub App installation token (or set keyless_exempt)"
                .into(),
        ),
        SaKey if matches!(c.provider.as_str(), "gsc" | "google" | "gcp") => Some(
            "service-account key: use workload identity federation (or set keyless_exempt)".into(),
        ),
        _ => None,
    }
}

fn rotate_step(sys: &SystemFile, c: &Credential, when: &str) -> HumanStep {
    let how = c
        .rotation
        .as_ref()
        .and_then(|r| r.command.clone().or_else(|| r.steps.first().cloned()));
    HumanStep {
        id: format!("rotate:{}", c.id),
        title: format!(
            "Rotate {} ({}, {when}): {}",
            c.id,
            c.provider,
            how.unwrap_or_else(|| "see SYSTEM.md".into())
        ),
        url: sys
            .provider(&c.provider)
            .and_then(|p| p.console.clone())
            .unwrap_or_default(),
        status: "pending".into(),
    }
}
